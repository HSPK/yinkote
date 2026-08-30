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
use yk_core::Error;
use yk_core::Key;

use super::{announce, key, notify_plugins, BadgeSort, ListParams};
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/items", get(list).post(create).delete(trash))
        .route("/libraries/:lib/items/:key", get(get_one).patch(update))
        .route("/libraries/:lib/items/:key/children", get(children))
        .route("/libraries/:lib/items/:key/notes/from-annotations", post(note_from_annotations))
        .route("/libraries/:lib/items/restore", post(restore))
        .route("/libraries/:lib/items/delete", post(destroy))
        .route("/libraries/:lib/trash", axum::routing::delete(empty_trash))
        .route(
            "/libraries/:lib/collections/:ckey/items",
            post(add_to_collection).delete(remove_from_collection),
        )
        .route("/libraries/:lib/duplicates", post(duplicates))
        .route("/libraries/:lib/duplicates", get(duplicate_groups))
        .route("/libraries/:lib/items/merge", post(merge))
}

/// Let plugins enrich drafts before anything is persisted.
///
/// One dispatch for the whole batch, not one per item: importing a library
/// means tens of thousands of drafts, and a round-trip each would hold the
/// request — and behind it the write lock — for minutes. A batch API that
/// degenerates into N calls is not a batch API.
///
/// Plugins answer with `patches`, aligned by position; a shorter array or a
/// null entry simply means "nothing for that one".
async fn enrich(app: &App, lib: i64, drafts: &mut [ItemDraft]) {
    if drafts.is_empty() {
        return;
    }
    let payload = json!({
        "libraryId": lib,
        "items": drafts.iter().map(item_preview).collect::<Vec<_>>(),
    });

    for outcome in app.plugins.dispatch(HookEvent::new(hooks::ITEM_BEFORE_CREATE, payload)).await {
        let Some(patches) = outcome.result.get("patches").and_then(|v| v.as_array()) else {
            continue;
        };
        for (draft, patch) in drafts.iter_mut().zip(patches) {
            if let Some(fields) = patch.get("fields").and_then(|v| v.as_object()) {
                // Never overwrite what the caller supplied: a plugin fills gaps.
                for (k, v) in fields {
                    draft.fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            if let Some(tags) = patch.get("tags").and_then(|v| v.as_array()) {
                for tag in tags.iter().filter_map(|t| t.as_str()) {
                    draft.tags.push(ItemTag::automatic(tag));
                }
            }
        }
    }
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

    // A query with no words is a filter, and a filter is the store's job: it
    // pages from an index and counts exactly. Sending `tag:survey` down the
    // ranked path instead answered "at least 2" for a tag on 28,763 items,
    // because the ranked path only reads as far as the page needs.
    //
    // Only when the whole query fits in an `ItemFilter`: `year:` and `author:`
    // have no field there and are applied after retrieval, so treating them as
    // filters would quietly match everything.
    let mut query = params.query(lib)?;
    let parsed = yk_search::parse::ParsedQuery::parse(&text);
    if parsed.is_fully_filterable() {
        parsed.apply_to(&mut query.filter);
        let page = app.store().items.list(&query).await?;
        return Ok((version_header(version), Json(page.map(ItemView::from))));
    }
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

    let found = hits.total;
    let capped = hits.capped;
    let hits = hits.hits;
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

    // What the search matched, not what this page holds. Reporting the page
    // length told the client `items.length >= total` after the first screen,
    // and its infinite scroll stopped there: a search could never show more
    // than one page of results.
    let total = found;
    let page = Page::new(views, total, query.offset, query.limit);
    // Marked when a retriever filled its pool: the client then shows "300+"
    // rather than a figure it would be reasonable to read as exact.
    let page = if capped { page.approximate() } else { page };
    // Relevance order, whatever sort was asked for. Said out loud so the table
    // stops drawing an arrow on a column it is not sorted by.
    let page = page.ranked();
    Ok((version_header(version), Json(page)))
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

    enrich(&app, lib, &mut drafts).await;

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
    // Files first: once the rows are gone there is nothing left to say which
    // directories belonged to them, and the bytes would sit there forever.
    super::files::forget_files(&app, lib, &keys).await;
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
    let doomed: Vec<Key> = app
        .store()
        .items
        .list(&yk_core::query::ItemQuery {
            filter: yk_core::query::ItemFilter {
                library_id: lib,
                trash: yk_core::query::TrashScope::Only,
                ..Default::default()
            },
            limit: u32::MAX,
            ..Default::default()
        })
        .await?
        .items
        .into_iter()
        .map(|i| i.key)
        .collect();
    super::files::forget_files(&app, lib, &doomed).await;

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

    // Filing a paper means meaning to read it, so anything here without a copy
    // gets one fetched. Read back rather than trusting the request: the caller
    // sent keys, and whether a file is already held is the store's answer.
    let mut filed = Vec::with_capacity(keys.len());
    for k in &keys {
        if let Ok(item) = app.store().items.get(lib, k).await {
            filed.push(item);
        }
    }
    let queued = super::files::queue_missing_files(app.store(), lib, &filed).await;

    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    Ok(Json(json!({ "added": n, "queued": queued, "version": version })))
}

/// Take items out of a collection without deleting them.
///
/// The store has been able to do this all along and only the assistant could
/// ask: there was no route, so the workbench could file an item into a
/// collection and never take it out again. A one-way door in the one screen
/// whose whole purpose is organising.
async fn remove_from_collection(
    State(app): State<App>,
    Path((lib, ckey)): Path<(i64, String)>,
    Json(body): Json<KeysBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let keys = parse_keys(&body.keys)?;
    let n = app.store().items.remove_from_collection(lib, &key(&ckey)?, &keys).await?;
    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    // Named `removed`, not `deleted`: the items are still in the library and
    // the difference is the entire point of the operation.
    Ok(Json(json!({ "removed": n, "version": version })))
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


/// How many duplicate groups one screen asks for.
///
/// A library with more duplicate groups than this has a bigger problem than
/// this screen can solve in one pass, and the user works through them a page at
/// a time regardless.
const DUPLICATE_GROUPS: u32 = 200;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct GroupParams {
    limit: Option<u32>,
}

/// The duplicates in the library, grouped.
async fn duplicate_groups(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(params): Query<GroupParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(DUPLICATE_GROUPS).min(DUPLICATE_GROUPS);
    let groups = app.store().items.duplicate_groups(lib, limit).await?;
    Ok(Json(json!({
        "groups": groups,
        // What the screen leads with: "you have 31 of these" is the answer, and
        // the groups are the evidence.
        "total": groups.len(),
    })))
}

#[derive(Deserialize)]
struct MergeBody {
    /// The record to keep.
    master: String,
    /// The records to fold into it.
    others: Vec<String>,
}

/// Fold duplicates into one record.
///
/// The losers go to the trash rather than being destroyed: a merge is the one
/// operation here that a user cannot undo by hand, and "it took my PDF" is not
/// something to find out about a week later.
async fn merge(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<MergeBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let master = key(&body.master)?;
    let others = parse_keys(&body.others)?;
    let item = app.store().items.merge(lib, &master, &others).await?;

    let mut keys = vec![master.clone()];
    keys.extend(others.iter().cloned());
    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    Ok(Json(json!({ "item": item, "merged": others.len(), "version": version })))
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

#[derive(Deserialize, Default)]
#[serde(default)]
struct FromAnnotations {
    /// Only these, or every annotation on the paper when empty.
    #[serde(rename = "annotationKeys")]
    annotation_keys: Vec<String>,
}

/// Gather what was highlighted on a paper into a note.
///
/// Annotations hang off the *attachment* they were drawn on, not the paper, so
/// this walks one level down to find them — asking for the paper's own children
/// would come back with the PDF and nothing else, which is the obvious version
/// of this endpoint and returns an empty note.
async fn note_from_annotations(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<FromAnnotations>,
) -> ApiResult<Json<serde_json::Value>> {
    let paper_key = key(&k)?;
    let paper = app.store().items.get(lib, &paper_key).await?;

    let wanted: Vec<String> = body.annotation_keys.iter().map(|s| s.trim().to_string()).collect();
    let mut marks = Vec::new();
    for child in app.store().items.children(lib, &paper_key).await? {
        if child.item_type != "attachment" {
            continue;
        }
        for grandchild in app.store().items.children(lib, &child.key).await? {
            if let Some(mark) = crate::notes::Annotation::of(&grandchild) {
                if wanted.is_empty() || wanted.contains(&mark.key) {
                    marks.push(mark);
                }
            }
        }
    }

    if marks.is_empty() {
        return Err(Error::invalid("this paper has no annotations to gather").into());
    }
    let count = marks.len();
    let html = crate::notes::render(paper.title(), &marks);

    let mut draft = ItemDraft::new("note");
    draft.parent_key = Some(paper_key.clone());
    draft.fields.insert("note".into(), html.into());
    let note = app.store().items.create(lib, draft).await?;

    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![paper_key.clone(), note.key.clone()],
        version,
    })
    .await?;

    Ok(Json(json!({ "note": note, "annotations": count, "version": version })))
}
