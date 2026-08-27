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
use yk_ai::ChatMessage;
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
        .route("/libraries/:lib/conversations/:key/run", get(run_state))
        .route("/libraries/:lib/conversations/:key/cancel", post(cancel_run))
}

/// Whether asking is possible, so the UI can explain itself rather than
/// offering a box that fails on submit.
async fn status(State(app): State<App>) -> Json<serde_json::Value> {
    let agent = &app.config.agent;
    Json(json!({
        "configured": agent.is_configured(),
        "model": agent.model,
        "endpoint": agent.endpoint,
        // What it may do, so that "can it change my library?" is answerable
        // without reading the source.
        "tools": app
            .agent()
            .map(|a| a.tool_names())
            .unwrap_or_default(),
        "writes": crate::agent::ACTIONS
            .iter()
            .filter(|a| a.writes())
            .map(|a| a.name())
            .collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
struct AskBody {
    content: String,
}

/// Record the question and start the turn.
///
/// Returns as soon as the run exists rather than when it finishes. A turn that
/// takes half a minute must not be tied to one HTTP request: switching tabs,
/// reloading, or a dropped connection would otherwise throw away work the model
/// is going to do anyway. Progress arrives on the event bus; the state is
/// readable at `GET …/run`, which is what lets a fresh page rejoin.
async fn ask(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<AskBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let question = body.content.trim().to_string();
    if question.is_empty() {
        return Err(Error::invalid("a question must not be empty").into());
    }
    // Stored before anything else can go wrong. What the user typed is theirs,
    // and it must survive a missing model, a timeout, or the server dying
    // mid-turn — a test holds this, and it caught me reordering it.
    app.store()
        .conversations
        .append(
            lib,
            &key,
            MessageDraft { role: "user".into(), content: question.clone(), meta: None },
        )
        .await?;

    if app.agent().is_none() {
        return Err(Error::invalid(
            "no model is configured; set agent.endpoint and agent.model",
        )
        .into());
    }

    // Refused rather than queued: two turns in one conversation would
    // interleave their tool calls into a transcript nobody can read.
    let run = app
        .runs
        .start(app.events(), lib, key.as_str(), &question)
        .ok_or_else(|| Error::invalid("this conversation is already answering"))?;

    let worker = app.clone();
    let conversation = key.clone();
    tokio::spawn(async move {
        run_turn(worker, lib, conversation, run).await;
    });

    Ok(Json(json!({ "started": true })))
}

/// Run the turn to its end, whatever happens to whoever asked for it.
async fn run_turn(app: App, lib: i64, key: yk_core::Key, run: std::sync::Arc<crate::runs::Run>) {
    let Some(agent) = app.agent() else { return };

    let history = match app.store().conversations.messages(lib, &key).await {
        Ok(messages) => messages.iter().map(to_chat).collect(),
        Err(e) => return run.fail(e.to_string()),
    };

    let progress = crate::runs::RunProgress {
        run: run.clone(),
        writers: crate::agent::ACTIONS.iter().filter(|a| a.writes()).map(|a| a.name()).collect(),
    };

    let turn = match agent.run_with(lib, history, &progress).await {
        Ok(turn) => turn,
        Err(e) => return run.fail(e.to_string()),
    };

    run.finish(turn.reply.clone(), turn.truncated, turn.stopped);

    // The steps are persisted from the run rather than rebuilt from the
    // transcript, so a turn watched live and one read back tomorrow are the
    // same thing rather than two renderings that can drift.
    let state = run.snapshot();
    let stored = app
        .store()
        .conversations
        .append(
            lib,
            &key,
            MessageDraft {
                role: "assistant".into(),
                content: turn.reply,
                meta: Some(json!({
                    "model": agent.model(),
                    "truncated": turn.truncated,
                    "stopped": turn.stopped,
                    "trace": crate::runs::steps_json(&state),
                })),
            },
        )
        .await;

    if let Err(e) = stored {
        tracing::warn!(error = %e, "could not store the agent's answer");
    }
    app.runs.forget_finished(key.as_str());
}

/// What a conversation's turn is doing, for a client that just arrived.
async fn run_state(
    State(app): State<App>,
    Path((_lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    // A whole state even when there is no run. `{"running": false}` is the same
    // type with holes in it, and the client that trusted the type crashed on
    // the first missing field — a half-shaped object is a lie told in JSON.
    Ok(Json(match app.runs.get(key.as_str()) {
        Some(run) => json!(run.snapshot()),
        None => json!(crate::runs::RunState::default()),
    }))
}

/// Ask the turn to stop at its next step.
///
/// Not mid-request: a call already in flight is going to arrive whatever we do,
/// and discarding its answer would only mean paying for it again.
async fn cancel_run(
    State(app): State<App>,
    Path((_lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let stopping = match app.runs.get(key.as_str()) {
        Some(run) => {
            run.cancel.stop();
            true
        }
        None => false,
    };
    Ok(Json(json!({ "stopping": stopping })))
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
