//! Collections and tags.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::*;

use super::{announce, key, ListParams};
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/collections", get(list).post(create))
        .route(
            "/libraries/:lib/collections/:key",
            get(get_one).patch(update).delete(remove),
        )
        .route("/libraries/:lib/tags", get(tags).patch(rename_tag).delete(delete_tag))
        .route("/libraries/:lib/tags/color", post(set_tag_color))
        .route("/libraries/:lib/facets", get(facets))
}

async fn list(State(app): State<App>, Path(lib): Path<i64>) -> ApiResult<Json<Vec<Collection>>> {
    Ok(Json(app.store().collections.list(lib).await?))
}

async fn get_one(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<Collection>> {
    Ok(Json(app.store().collections.get(lib, &key(&k)?).await?))
}

async fn create(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(draft): Json<CollectionDraft>,
) -> ApiResult<Json<Collection>> {
    let c = app.store().collections.create(lib, draft).await?;
    app.events().publish(DomainEvent::CollectionsChanged { library_id: lib, version: c.version });
    Ok(Json(c))
}

async fn update(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(patch): Json<CollectionPatch>,
) -> ApiResult<Json<Collection>> {
    let c = app.store().collections.update(lib, &key(&k)?, patch).await?;
    app.events().publish(DomainEvent::CollectionsChanged { library_id: lib, version: c.version });
    Ok(Json(c))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct DeleteParams {
    recursive: bool,
}

async fn remove(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Query(p): Query<DeleteParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().collections.delete(lib, &key(&k)?, p.recursive).await?;
    let version =
        announce(&app, lib, |version| DomainEvent::CollectionsChanged { library_id: lib, version })
            .await?;
    Ok(Json(json!({ "deleted": n, "version": version })))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct TagParams {
    q: Option<String>,
    limit: Option<u32>,
}

async fn tags(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(p): Query<TagParams>,
) -> ApiResult<Json<Vec<Tag>>> {
    let limit = p.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(app.store().tags.list(lib, p.q.as_deref(), limit).await?))
}

/// How many facets a caller gets when it does not say.
///
/// Defined here rather than in the client, and used by the startup warm-up,
/// so there is one number. Two — a client asking for eighty and a warm-up
/// computing sixty — is a cache that fills a slot nobody asks for, which is
/// exactly what happened.
pub const FACET_LIMIT: u32 = 80;

/// Tags that co-occur with the current filter, for progressive narrowing.
async fn facets(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<Vec<Tag>>> {
    let filter = params.filter(lib)?;
    let limit = params.limit.unwrap_or(FACET_LIMIT).clamp(1, 500);
    Ok(Json(app.store().tags.facets(&filter, limit).await?))
}

#[derive(Deserialize)]
struct RenameBody {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct TagColour {
    name: String,
    /// A palette name, or empty to go back to the colour derived from the name.
    #[serde(default)]
    color: String,
}

/// Give a tag a colour, or take one away.
///
/// Clearing is not the same as grey: a tag with no stored colour still draws in
/// one derived from its name, because a library where colours only exist after
/// somebody assigns two hundred of them is a library with no colours.
async fn set_tag_color(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<TagColour>,
) -> ApiResult<Json<serde_json::Value>> {
    let chosen = body.color.trim();
    app.store()
        .tags
        .set_color(lib, body.name.trim(), (!chosen.is_empty()).then_some(chosen))
        .await?;

    let version = announce(&app, lib, |_| DomainEvent::TagsChanged { library_id: lib }).await?;
    Ok(Json(json!({ "name": body.name, "color": chosen, "version": version })))
}

async fn rename_tag(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<RenameBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().tags.rename(lib, &body.from, &body.to).await?;
    app.events().publish(DomainEvent::TagsChanged { library_id: lib });
    // Renaming rewrites indexed text on every affected item.
    let search = app.search().clone();
    tokio::spawn(async move {
        let _ = search.reindex(lib).await;
    });
    Ok(Json(json!({ "updated": n })))
}

#[derive(Deserialize)]
struct TagBody {
    name: String,
}

async fn delete_tag(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<TagBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().tags.delete(lib, &body.name).await?;
    app.events().publish(DomainEvent::TagsChanged { library_id: lib });
    Ok(Json(json!({ "deleted": n })))
}
