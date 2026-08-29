//! Smart collections.
//!
//! A smart collection is a saved search string, so evaluating one is exactly
//! the `/items?q=…` pipeline with the stored query substituted. There is no
//! second matching engine here on purpose: anything the search box can express,
//! a smart collection can save, and the two can never disagree.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::*;
use yk_core::query::{ItemFilter, SearchMode, SearchRequest};
use yk_store::counts::Answer;

use super::{announce, key, ListParams};
use crate::error::ApiResult;
use crate::state::App;

/// Counting a text query means running it; cap the page so a sidebar refresh
/// stays cheap.
///
/// This bounds the *page*, not the count. The count comes from the fused
/// candidate list, which the retrievers cap lower still — a keyword query
/// tops out around three hundred however many documents match. Whether that
/// happened is what [`count_of`] returns alongside the number, because a bare
/// "300" beside a saved search that matches twenty thousand is a wrong answer
/// rather than a rounded one.
const COUNT_CAP: u32 = 500;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/smart-collections", get(list).post(create))
        .route(
            "/libraries/:lib/smart-collections/:key",
            get(get_one).patch(update).delete(remove),
        )
        .route("/libraries/:lib/smart-collections/:key/items", get(items))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct ListParamsWithCounts {
    /// Evaluating counts costs one query each; the sidebar asks for them, a
    /// picker does not.
    counts: bool,
}

/// Turn a stored query into the same parameters the item list understands.
fn to_params(smart: &SmartCollection) -> ListParams {
    ListParams {
        q: Some(smart.query.clone()).filter(|q| !q.trim().is_empty()),
        mode: Some(smart.mode.clone()),
        sort: Some(smart.sort.clone()),
        direction: Some(smart.direction.clone()),
        ..Default::default()
    }
}

/// How many items a saved search holds, and whether that number is a floor.
async fn count_of(app: &App, lib: i64, smart: &SmartCollection) -> ApiResult<(i64, bool)> {
    let params = to_params(smart);
    let filter = params.filter(lib)?;

    // Whether there are *words* left after the operators have been taken out,
    // not whether the query string is empty. `tag:survey` has a query and no
    // words: it is a filter, and a filter can be counted exactly through the
    // index. Testing the raw string sent it through the ranked path instead,
    // where it came back capped at the page size — 501 for a tag on 28,757
    // items, and presented as a figure.
    let mut parsed_filter = filter.clone();
    let parsed = yk_search::parse::ParsedQuery::parse(params.text());
    parsed.apply_to(&mut parsed_filter);
    if parsed.is_fully_filterable() {
        return Ok((app.store().items.count(&parsed_filter).await?, false));
    }

    let hits = app
        .search()
        .search(&SearchRequest {
            text: params.text().to_string(),
            mode: params.search_mode(),
            filter,
            limit: COUNT_CAP,
            offset: 0,
            highlight: false,
        })
        .await?;
    Ok((hits.total, hits.capped))
}

async fn list(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(p): Query<ListParamsWithCounts>,
) -> ApiResult<Json<Vec<SmartCollection>>> {
    if !p.counts {
        return Ok(Json(app.store().smart.list(lib).await?));
    }

    // A saved search's count can only change when the library does, so the
    // library version is the key.
    let version = app.store().libraries.version(lib).await.unwrap_or(-1);
    let cache_key = format!("smart:{lib}");
    match app.smart_counts.look_up(&cache_key, version) {
        Answer::Fresh(cached) => Ok(Json(cached)),
        // The previous numbers, while fresh ones are worked out behind this
        // request. Counting a saved search means *running* it, so any edit
        // anywhere would otherwise put the sidebar behind a full search per
        // saved query — 85ms on the first navigation after every change, for
        // labels beside names. Several of them already read "734+".
        Answer::Stale(cached) => {
            if app.smart_counts.claim(&cache_key) {
                let app = app.clone();
                let key = cache_key.clone();
                tokio::spawn(async move {
                    let _ = recount(&app, lib, key.clone(), version).await;
                    app.smart_counts.release(&key);
                });
            }
            Ok(Json(cached))
        }
        // Nothing to show: a cold cache must be correct, not merely quick.
        Answer::Missing => Ok(Json(recount(&app, lib, cache_key, version).await?)),
    }
}

/// Count every saved search, and remember the answer with what it cost.
///
/// Counted together rather than one after another: each is a full search, so
/// awaiting them in turn made the sidebar wait for the sum of every saved
/// search the user has ever kept. A broken query must not break the sidebar,
/// so it reports as empty.
async fn recount(
    app: &App,
    lib: i64,
    cache_key: String,
    version: i64,
) -> ApiResult<Vec<SmartCollection>> {
    let started = std::time::Instant::now();
    let mut collections = app.store().smart.list(lib).await?;

    let running: Vec<_> = collections
        .iter()
        .cloned()
        .map(|smart| {
            let app = app.clone();
            tokio::spawn(async move { count_of(&app, lib, &smart).await.unwrap_or((0, false)) })
        })
        .collect();
    for (smart, handle) in collections.iter_mut().zip(running) {
        let (count, approximate) = handle.await.unwrap_or((0, false));
        smart.item_count = Some(count);
        smart.item_count_approximate = approximate;
    }
    if version >= 0 {
        app.smart_counts.put_timed(cache_key, version, collections.clone(), started.elapsed());
    }
    Ok(collections)
}

async fn get_one(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<SmartCollection>> {
    let mut smart = app.store().smart.get(lib, &key(&k)?).await?;
    let (count, approximate) = count_of(&app, lib, &smart).await.unwrap_or((0, false));
    smart.item_count = Some(count);
    smart.item_count_approximate = approximate;
    Ok(Json(smart))
}

/// Resolve a smart collection to actual items, so a client can follow it
/// without knowing the query language.
async fn items(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Query(overrides): Query<ListParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let smart = app.store().smart.get(lib, &key(&k)?).await?;
    let mut params = to_params(&smart);
    // Paging is the caller's business; everything else comes from the definition.
    params.limit = overrides.limit;
    params.offset = overrides.offset;

    let query = params.query(lib)?;
    if params.text().is_empty() {
        let page = app.store().items.list(&query).await?;
        return Ok(Json(json!({
            "items": page.items, "total": page.total,
            "offset": page.offset, "limit": page.limit,
        })));
    }

    let hits = app
        .search()
        .search(&SearchRequest {
            text: params.text().to_string(),
            mode: params.search_mode(),
            filter: ItemFilter { library_id: lib, ..query.filter },
            limit: query.limit,
            offset: query.offset,
            highlight: true,
            })
        .await?;
    let keys: Vec<yk_core::Key> = hits.hits.iter().map(|h| h.key.clone()).collect();
    let items = app.store().items.get_many(lib, &keys).await?;
    Ok(Json(json!({
        "items": items, "total": items.len(),
        "offset": query.offset, "limit": query.limit,
    })))
}

async fn create(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(draft): Json<SmartCollectionDraft>,
) -> ApiResult<Json<SmartCollection>> {
    validate_mode(draft.mode.as_deref())?;
    let mut created = app.store().smart.create(lib, draft).await?;
    let (count, approximate) = count_of(&app, lib, &created).await.unwrap_or((0, false));
    created.item_count = Some(count);
    created.item_count_approximate = approximate;
    announce(&app, lib, |version| DomainEvent::CollectionsChanged { library_id: lib, version })
        .await?;
    Ok(Json(created))
}

async fn update(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(patch): Json<SmartCollectionPatch>,
) -> ApiResult<Json<SmartCollection>> {
    validate_mode(patch.mode.as_deref())?;
    let mut updated = app.store().smart.update(lib, &key(&k)?, patch).await?;
    let (count, approximate) = count_of(&app, lib, &updated).await.unwrap_or((0, false));
    updated.item_count = Some(count);
    updated.item_count_approximate = approximate;
    announce(&app, lib, |version| DomainEvent::CollectionsChanged { library_id: lib, version })
        .await?;
    Ok(Json(updated))
}

async fn remove(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().smart.delete(lib, &key(&k)?).await?;
    let version =
        announce(&app, lib, |version| DomainEvent::CollectionsChanged { library_id: lib, version })
            .await?;
    Ok(Json(json!({ "deleted": n, "version": version })))
}

fn validate_mode(mode: Option<&str>) -> ApiResult<()> {
    match mode {
        Some(m) if SearchMode::parse(m).is_none() => {
            Err(yk_core::Error::invalid(format!("unknown search mode '{m}'")).into())
        }
        _ => Ok(()),
    }
}
