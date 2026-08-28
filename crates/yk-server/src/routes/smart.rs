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

use super::{announce, key, ListParams};
use crate::error::ApiResult;
use crate::state::App;

/// Counting a text query means running it; cap the work so a sidebar refresh
/// stays cheap. Anything at the cap is reported as "500+".
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
#[serde(default)]
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

async fn count_of(app: &App, lib: i64, smart: &SmartCollection) -> ApiResult<i64> {
    let params = to_params(smart);
    let filter = params.filter(lib)?;

    if params.text().is_empty() {
        // Pure filter: exact and index-backed.
        return Ok(app.store().items.count(&filter).await?);
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
    Ok(hits.total)
}

async fn list(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(p): Query<ListParamsWithCounts>,
) -> ApiResult<Json<Vec<SmartCollection>>> {
    let mut collections = app.store().smart.list(lib).await?;
    if p.counts {
        // Remembered against the library version: a saved search's count can
        // only change when the library does, and the sidebar asks for every
        // one of them on every navigation.
        let version = app.store().libraries.version(lib).await.unwrap_or(-1);
        let cache_key = format!("smart:{lib}");
        if let Some(cached) = app.smart_counts.get(&cache_key, version) {
            return Ok(Json(cached));
        }

        // Counted together rather than one after another. Each is a full search
        // — 21ms — so awaiting them in turn made the sidebar wait for the sum
        // of every saved search the user has ever kept.
        //
        // A broken query must not break the sidebar; it reports as empty.
        let running: Vec<_> = collections
            .iter()
            .cloned()
            .map(|smart| {
                let app = app.clone();
                tokio::spawn(async move { count_of(&app, lib, &smart).await.unwrap_or(0) })
            })
            .collect();
        for (smart, handle) in collections.iter_mut().zip(running) {
            smart.item_count = Some(handle.await.unwrap_or(0));
        }
        if version >= 0 {
            app.smart_counts.put(cache_key, version, collections.clone());
        }
    }
    Ok(Json(collections))
}

async fn get_one(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<SmartCollection>> {
    let mut smart = app.store().smart.get(lib, &key(&k)?).await?;
    smart.item_count = Some(count_of(&app, lib, &smart).await.unwrap_or(0));
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
    created.item_count = Some(count_of(&app, lib, &created).await.unwrap_or(0));
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
    updated.item_count = Some(count_of(&app, lib, &updated).await.unwrap_or(0));
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
