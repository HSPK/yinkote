//! Item CRUD, batch writes, trash and collection membership.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::*;
use yk_core::plugin::{hooks, HookEvent};
use yk_core::query::{SearchHit, SearchRequest};
use yk_core::Key;

use super::{announce, key, notify_plugins, BadgeSort, ListParams};
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/items", get(list).post(create).delete(trash))
        .route("/libraries/:lib/items/:key", get(get_one).patch(update))
        .route("/libraries/:lib/items/:key/children", get(children))
        .route("/libraries/:lib/items/restore", post(restore))
        .route("/libraries/:lib/items/delete", post(destroy))
        .route("/libraries/:lib/trash", axum::routing::delete(empty_trash))
        .route("/libraries/:lib/collections/:ckey/items", post(add_to_collection))
        .route("/libraries/:lib/duplicates", post(duplicates))
}

/// An item plus, when the request was a search, why it matched.
#[derive(Serialize)]
pub struct ItemView {
    #[serde(flatten)]
    item: Item,
    #[serde(rename = "match", skip_serializing_if = "Option::is_none")]
    hit: Option<SearchHit>,
}

impl From<Item> for ItemView {
    fn from(item: Item) -> Self {
        Self { item, hit: None }
    }
}

fn version_header(version: i64) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(v) = version.to_string().parse() {
        h.insert("Last-Modified-Version", v);
    }
    h
}

/// How many items a badge sort will consider.
///
/// Badge values come from a plugin, not the database, so ordering by one means
/// asking the plugin about every candidate. That is fine for a working set and
/// unreasonable for a whole library, so the sort is bounded and says so in a
/// header rather than quietly returning a wrong order.
const BADGE_SORT_CAP: u32 = 2_000;

/// List or search, depending on whether `q` is present.
///
/// Keeping both behind one endpoint means the UI has a single code path and
/// filters compose with the query for free.
async fn list(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(params): Query<ListParams>,
) -> ApiResult<(HeaderMap, Json<Page<ItemView>>)> {
    let version = app.store().libraries.version(lib).await?;
    let text = params.text().to_string();

    if let Some(badge) = params.badge_sort() {
        return list_by_badge(app, lib, params, badge, version).await;
    }

    if text.is_empty() {
        let page = app.store().items.list(&params.query(lib)?).await?;
        return Ok((version_header(version), Json(page.map(ItemView::from))));
    }

    let query = params.query(lib)?;
    let hits = app
        .search()
        .search(&SearchRequest {
            text,
            mode: params.search_mode(),
            filter: query.filter.clone(),
            limit: query.limit,
            offset: query.offset,
            highlight: true,
        })
        .await?;

    let keys: Vec<Key> = hits.iter().map(|h| h.key.clone()).collect();
    let items = app.store().items.get_many(lib, &keys).await?;
    let by_key: HashMap<String, Item> =
        items.into_iter().map(|i| (i.key.to_string(), i)).collect();

    // Preserve relevance order from the search engine.
    let views: Vec<ItemView> = hits
        .into_iter()
        .filter_map(|hit| {
            by_key
                .get(hit.key.as_str())
                .cloned()
                .map(|item| ItemView { item, hit: Some(hit) })
        })
        .collect();

    let total = views.len() as i64 + query.offset as i64;
    Ok((
        version_header(version),
        Json(Page::new(views, total, query.offset, query.limit)),
    ))
}

async fn get_one(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<Item>> {
    Ok(Json(app.store().items.get(lib, &key(&k)?).await?))
}

async fn children(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<Vec<Item>>> {
    Ok(Json(app.store().items.children(lib, &key(&k)?).await?))
}

/// Accepts either a single draft or an array, and always answers with per-row
/// results so one bad item never fails the batch.
#[derive(Deserialize)]
#[serde(untagged)]
enum CreateBody {
    One(Box<ItemDraft>),
    Many(Vec<ItemDraft>),
}

async fn create(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<CreateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut drafts = match body {
        CreateBody::One(d) => vec![*d],
        CreateBody::Many(v) => v,
    };

    // Let plugins enrich or veto drafts before anything is persisted.
    for draft in &mut drafts {
        let payload = json!({ "libraryId": lib, "item": item_preview(draft) });
        for outcome in
            app.plugins.dispatch(HookEvent::new(hooks::ITEM_BEFORE_CREATE, payload)).await
        {
            if let Some(patch) = outcome.result.get("fields").and_then(|v| v.as_object()) {
                for (k, v) in patch {
                    draft.fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            if let Some(tags) = outcome.result.get("tags").and_then(|v| v.as_array()) {
                for t in tags.iter().filter_map(|t| t.as_str()) {
                    draft.tags.push(ItemTag::automatic(t));
                }
            }
        }
    }

    let results = app.store().items.create_many(lib, drafts).await?;
    let created: Vec<&Item> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    let keys: Vec<Key> = created.iter().map(|i| i.key.clone()).collect();

    let version = if keys.is_empty() {
        app.store().libraries.version(lib).await?
    } else {
        let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
            library_id: lib,
            keys: keys.clone(),
            version,
        })
        .await?;
        notify_plugins(&app, hooks::ITEM_CREATED, json!({ "libraryId": lib, "items": created }));
        version
    };

    Ok(Json(json!({
        "version": version,
        "created": created,
        "failed": results
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().err().map(|e| json!({
                "index": i, "code": e.code(), "message": e.to_string()
            })))
            .collect::<Vec<_>>(),
    })))
}

fn item_preview(draft: &ItemDraft) -> serde_json::Value {
    json!({
        "itemType": draft.item_type,
        "fields": draft.fields,
        "creators": draft.creators.iter().map(Creator::display).collect::<Vec<_>>(),
    })
}

async fn update(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    headers: HeaderMap,
    Json(patch): Json<ItemPatch>,
) -> ApiResult<Json<Item>> {
    let if_version = headers
        .get("If-Unmodified-Since-Version")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok());

    let item = app.store().items.update(lib, &key(&k)?, patch, if_version).await?;
    app.events().publish(DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![item.key.clone()],
        version: item.version,
    });
    notify_plugins(&app, hooks::ITEM_UPDATED, json!({ "libraryId": lib, "item": item }));
    Ok(Json(item))
}

#[derive(Deserialize)]
struct KeysBody {
    keys: Vec<String>,
}

fn parse_keys(raw: &[String]) -> yk_core::Result<Vec<Key>> {
    raw.iter().map(|k| key(k)).collect()
}

async fn trash(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<KeysBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let keys = parse_keys(&body.keys)?;
    let n = app.store().items.set_trashed(lib, &keys, true).await?;
    let version = announce(&app, lib, |version| DomainEvent::ItemsTrashed {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    notify_plugins(&app, hooks::ITEM_TRASHED, json!({ "libraryId": lib, "keys": keys }));
    Ok(Json(json!({ "trashed": n, "version": version })))
}

async fn restore(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<KeysBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let keys = parse_keys(&body.keys)?;
    let n = app.store().items.set_trashed(lib, &keys, false).await?;
    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    Ok(Json(json!({ "restored": n, "version": version })))
}

async fn destroy(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<KeysBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let keys = parse_keys(&body.keys)?;
    let n = app.store().items.delete(lib, &keys).await?;
    let version = announce(&app, lib, |version| DomainEvent::ItemsDeleted {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    Ok(Json(json!({ "deleted": n, "version": version })))
}

async fn empty_trash(
    State(app): State<App>,
    Path(lib): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().items.empty_trash(lib).await?;
    let version = announce(&app, lib, |version| DomainEvent::ItemsDeleted {
        library_id: lib,
        keys: Vec::new(),
        version,
    })
    .await?;
    Ok(Json(json!({ "deleted": n, "version": version })))
}

async fn add_to_collection(
    State(app): State<App>,
    Path((lib, ckey)): Path<(i64, String)>,
    Json(body): Json<KeysBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let keys = parse_keys(&body.keys)?;
    let n = app.store().items.add_to_collection(lib, &key(&ckey)?, &keys).await?;
    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    Ok(Json(json!({ "added": n, "version": version })))
}

#[derive(Deserialize)]
struct DuplicateBody {
    fingerprints: Vec<String>,
}

async fn duplicates(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<DuplicateBody>,
) -> ApiResult<Json<Vec<Item>>> {
    Ok(Json(app.store().items.find_by_fingerprint(lib, &body.fingerprints).await?))
}


/// List ordered by a plugin-supplied badge.
///
/// Items the plugin has nothing to say about sort last in either direction:
/// "no impact factor" is not the same as "the lowest impact factor", and
/// burying real values under blanks when sorting ascending would be useless.
async fn list_by_badge(
    app: App,
    lib: i64,
    params: ListParams,
    badge: BadgeSort,
    version: i64,
) -> ApiResult<(HeaderMap, Json<Page<ItemView>>)> {
    let mut query = params.query(lib)?;
    let (offset, limit) = (query.offset, query.limit);
    query.offset = 0;
    query.limit = BADGE_SORT_CAP;

    let page = app.store().items.list(&query).await?;
    let total = page.total;
    let mut items = page.items;

    let badges = app.badges.resolve(&items).await;
    let rank_of = |item: &Item| -> Option<f64> {
        badges.get(item.key.as_str())?.iter().find_map(|v| {
            (v.badge == badge.badge && v.plugin_id == badge.plugin_id).then_some(v.rank).flatten()
        })
    };

    let descending = params.descending();
    items.sort_by(|a, b| match (rank_of(a), rank_of(b)) {
        (Some(x), Some(y)) => {
            let ordering = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
            if descending { ordering.reverse() } else { ordering }
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let window: Vec<ItemView> = items
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(ItemView::from)
        .collect();

    let mut headers = version_header(version);
    if total > BADGE_SORT_CAP as i64 {
        if let Ok(v) = BADGE_SORT_CAP.to_string().parse() {
            headers.insert("X-Badge-Sort-Cap", v);
        }
    }
    Ok((headers, Json(Page::new(window, total, offset, limit))))
}
