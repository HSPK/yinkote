//! Conversation history.
//!
//! Storage only — the agent loop that will produce assistant turns lands
//! separately, and keeping these endpoints free of it means history is usable
//! (and testable) before any model is configured.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::model::{Conversation, Message, MessageDraft};

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/conversations", get(list).post(create))
        .route(
            "/libraries/:lib/conversations/:key",
            get(get_one).patch(rename).delete(remove),
        )
        .route("/libraries/:lib/conversations/:key/messages", get(messages).post(append))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListParams {
    limit: Option<u32>,
}

async fn list(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(p): Query<ListParams>,
) -> ApiResult<Json<Vec<Conversation>>> {
    Ok(Json(app.store().conversations.list(lib, p.limit.unwrap_or(100)).await?))
}

async fn get_one(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<Conversation>> {
    Ok(Json(app.store().conversations.get(lib, &key(&k)?).await?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CreateBody {
    title: Option<String>,
    scope: Option<String>,
}

async fn create(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<CreateBody>,
) -> ApiResult<Json<Conversation>> {
    // An untitled thread is normal — the first message usually names it. The
    // placeholder is left to the client, which is the only side that knows
    // which language the user reads.
    let title = body.title.as_deref().unwrap_or_default();
    Ok(Json(app.store().conversations.create(lib, title, body.scope.as_deref()).await?))
}

#[derive(Deserialize)]
struct RenameBody {
    title: String,
}

async fn rename(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<RenameBody>,
) -> ApiResult<Json<Conversation>> {
    Ok(Json(app.store().conversations.rename(lib, &key(&k)?, &body.title).await?))
}

async fn remove(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().conversations.delete(lib, &key(&k)?).await?;
    Ok(Json(json!({ "deleted": n })))
}

async fn messages(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<Vec<Message>>> {
    Ok(Json(app.store().conversations.messages(lib, &key(&k)?).await?))
}

async fn append(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(draft): Json<MessageDraft>,
) -> ApiResult<Json<Message>> {
    Ok(Json(app.store().conversations.append(lib, &key(&k)?, draft).await?))
}
