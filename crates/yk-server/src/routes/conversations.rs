//! Conversation history, and asking the agent a question.
//!
//! History works with no model configured — that is deliberate, and it is what
//! lets the transcript be useful and testable before anyone has pointed the
//! server at an endpoint.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::model::{Conversation, Message, MessageDraft};
use yk_core::ports::ChatMessage;
use yk_core::Error;

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
        .route("/libraries/:lib/conversations/:key/ask", post(ask))
        .route("/agent", get(status))
}

/// Whether asking is possible, so the UI can explain itself rather than
/// offering a box that fails on submit.
async fn status(State(app): State<App>) -> Json<serde_json::Value> {
    let agent = &app.config.agent;
    Json(json!({
        "configured": agent.is_configured(),
        "model": agent.model,
        "endpoint": agent.endpoint,
    }))
}

#[derive(Deserialize)]
struct AskBody {
    content: String,
}

/// Record the question, run the agent, record the answer.
///
/// The user's turn is persisted *before* the model is called: if the model
/// times out or the server dies mid-request, what they typed is still there.
async fn ask(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<AskBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let question = body.content.trim();
    if question.is_empty() {
        return Err(Error::invalid("a question must not be empty").into());
    }

    let store = app.store();
    store
        .conversations
        .append(lib, &key, MessageDraft { role: "user".into(), content: question.into(), meta: None })
        .await?;

    let agent = app.agent().ok_or_else(|| {
        Error::invalid("no model is configured; set agent.endpoint and agent.model")
    })?;

    let history = store.conversations.messages(lib, &key).await?;
    let turn = agent.run(lib, history.iter().map(to_chat).collect()).await?;

    let reply = store
        .conversations
        .append(
            lib,
            &key,
            MessageDraft {
                role: "assistant".into(),
                content: turn.reply.clone(),
                // The tool traffic is kept beside the answer so a reader can
                // see what it was based on, without cluttering the transcript.
                meta: Some(json!({
                    "model": agent.model(),
                    "truncated": turn.truncated,
                    "steps": turn.transcript,
                })),
            },
        )
        .await?;

    Ok(Json(json!({ "message": reply, "truncated": turn.truncated })))
}

/// A stored message as the model should see it.
///
/// Tool traffic is deliberately *not* replayed: it was noise specific to an
/// earlier question, and sending it again wastes context to no benefit.
fn to_chat(message: &Message) -> ChatMessage {
    ChatMessage::new(&message.role, message.content.clone())
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
