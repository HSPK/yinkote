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
        .route("/agent", get(status).put(configure))
        .route("/libraries/:lib/conversations/:key/run", get(run_state))
        .route("/libraries/:lib/conversations/:key/cancel", post(cancel_run))
        .route("/libraries/:lib/items/:key/conversations", get(about_item))
}

/// What has already been asked about one paper.
async fn about_item(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let item = key(&k)?;
    let found = app.store().conversations.mentioning(lib, &item).await?;
    Ok(Json(json!({ "conversations": found })))
}

/// Point the assistant at a model.
///
/// The program is a local server the user started; telling them to edit a
/// file and restart it would make the web interface a partial one. The agent
/// is rebuilt in place, so a conversation started a moment later uses the new
/// model without anything being restarted.
async fn configure(
    State(app): State<App>,
    Json(body): Json<AgentConfigBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = app.config();
    let blank = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or_default().is_empty();

    if let Some(endpoint) = &body.endpoint {
        config.agent.endpoint = Some(endpoint.trim().to_string()).filter(|e| !e.is_empty());
    }
    if let Some(model) = &body.model {
        config.agent.model = Some(model.trim().to_string()).filter(|m| !m.is_empty());
    }
    // An absent key leaves whatever is stored; an empty one clears it. Without
    // the distinction, a form that never shows the key would erase it on every
    // save.
    if let Some(key) = &body.api_key {
        config.agent.api_key = Some(key.trim().to_string()).filter(|k| !k.is_empty());
    }
    if let Some(allow) = body.allow_commands {
        config.agent.allow_commands = allow;
    }
    if let Some(steps) = body.max_steps {
        config.agent.max_steps = steps.clamp(1, 64);
    }
    if let Some(skills) = body.disabled_skills {
        config.agent.disabled_skills = skills;
    }
    if let Some(tools) = body.disabled_tools {
        config.agent.disabled_tools = tools;
    }

    if blank(&config.agent.endpoint) || blank(&config.agent.model) {
        // Saying which half is missing; "not configured" sends people to the
        // wrong field half the time.
        return Err(Error::invalid(match blank(&config.agent.endpoint) {
            true => "an endpoint is needed, e.g. http://127.0.0.1:11434/v1",
            false => "a model name is needed",
        })
        .into());
    }

    config.save()?;
    let rebuilt = crate::build_agent(&config, &app.services);
    let ok = rebuilt.is_some();
    *app.agent.write() = rebuilt;
    *app.config.write() = config;

    if !ok {
        return Err(Error::invalid("that endpoint could not be used").into());
    }
    Ok(Json(agent_status(&app)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentConfigBody {
    endpoint: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    allow_commands: Option<bool>,
    max_steps: Option<usize>,
    disabled_skills: Option<Vec<String>>,
    disabled_tools: Option<Vec<String>>,
}

/// Whether asking is possible, so the UI can explain itself rather than
/// offering a box that fails on submit.
async fn status(State(app): State<App>) -> Json<serde_json::Value> {
    Json(agent_status(&app))
}

/// What the workbench needs to know about the assistant.
///
/// Shared with `configure` so that saving a model answers with exactly what a
/// fresh read would say — a form that has to re-fetch to find out whether it
/// worked will eventually show a stale answer.
fn agent_status(app: &App) -> serde_json::Value {
    let config = app.config();
    let agent = &config.agent;
    json!({
        "configured": agent.is_configured(),
        "model": agent.model,
        "endpoint": agent.endpoint,
        // Whether one is set, never the key itself.
        "hasApiKey": agent.api_key.as_deref().is_some_and(|k| !k.is_empty()),
        "allowCommands": agent.allow_commands,
        "maxSteps": agent.max_steps,
        // What is installed and what is off, so the settings page can show
        // both without a second request or a second source of truth.
        "skills": crate::agent::skills::Skills::load_dir(&config.skills_dir())
            .iter()
            .map(|s| json!({
                "name": s.name,
                "description": s.description,
                "enabled": agent.skill_enabled(&s.name),
            }))
            .collect::<Vec<_>>(),
        "disabledTools": agent.disabled_tools,
        "allTools": app.tool_catalogue(),
        // What it may do, so that "can it change my library?" is answerable
        // without reading the source.
        "tools": app.agent().map(|a| a.tool_names()).unwrap_or_default(),
        "writes": crate::agent::ACTIONS
            .iter()
            .filter(|a| a.writes())
            .map(|a| a.name())
            .collect::<Vec<_>>(),
    })
}

#[derive(Deserialize)]
struct AskBody {
    content: String,
    /// Papers the user named with `@`. They are put in front of the model as
    /// facts rather than left for it to search for: the user has already said
    /// which ones they mean, and making it guess again wastes a step and
    /// sometimes finds a different paper.
    #[serde(default)]
    mentions: Vec<yk_core::Key>,
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
            MessageDraft { role: "user".into(), content: question.clone(), meta: None, mentions: body.mentions.clone() },
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

/// The conversation's own context: what it is about, and which papers it names.
///
/// Returns `None` when there is nothing to say, so an ordinary chat pays
/// nothing for the feature.
async fn turn_context(
    app: &App,
    lib: i64,
    key: &yk_core::Key,
    thread: &[yk_core::model::Message],
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Ok(conversation) = app.store().conversations.get(lib, key).await {
        if let Some(scope) = conversation.scope.as_deref().filter(|s| !s.is_empty()) {
            if let Ok(collection) = scope.parse() {
                if let Ok(found) = app.store().collections.get(lib, &collection).await {
                    parts.push(format!(
                        "This conversation is about the collection \"{}\" ({} items). Unless \
                         the user says otherwise, search inside it by passing collection: \
                         \"{}\" to search_library.",
                        found.name, found.item_count, scope
                    ));
                }
            }
        }
    }

    let mentioned = mentioned_items(app, lib, thread).await;
    if !mentioned.is_empty() {
        let listed = mentioned
            .iter()
            .map(|item| {
                serde_json::to_string(&crate::agent::summarise(item)).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!(
            "The user has referred to these papers by name. Treat them as the subject unless \
             they say otherwise; you do not need to search for them again:\n{listed}"
        ));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Every paper named in the conversation so far, newest mention winning.
async fn mentioned_items(
    app: &App,
    lib: i64,
    thread: &[yk_core::model::Message],
) -> Vec<yk_core::model::Item> {
    let mut keys: Vec<yk_core::Key> = Vec::new();
    for message in thread.iter().rev() {
        for k in &message.mentions {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        // A conversation that has ranged over forty papers is not asking about
        // forty papers; the recent ones are the ones in play.
        if keys.len() >= MENTION_CONTEXT {
            break;
        }
    }
    keys.truncate(MENTION_CONTEXT);
    app.store().items.get_many(lib, &keys).await.unwrap_or_default()
}

/// How many named papers to put in front of the model.
const MENTION_CONTEXT: usize = 8;

/// How much of a thread the model is shown.
///
/// Enough that a conversation feels continuous, few enough that a long one
/// does not cost more every turn. What was said an hour ago is in the
/// library, not in the prompt.
const HISTORY_TURNS: u32 = 40;

/// Run the turn to its end, whatever happens to whoever asked for it.
async fn run_turn(app: App, lib: i64, key: yk_core::Key, run: std::sync::Arc<crate::runs::Run>) {
    let Some(agent) = app.agent() else { return };

    // Read once and used twice: the transcript the model sees, and the papers
    // it names. Fetching the thread a second time to find the mentions would
    // double the cost of starting every turn.
    //
    // Bounded, because a prompt is not. A thread that has been going for a
    // week holds hundreds of messages and tool results of a hundred kilobytes
    // each; sending all of it re-reads the entire history on every turn and
    // eventually exceeds whatever context the model has. The recent end is
    // what the next answer depends on — the standing facts (what the
    // conversation is scoped to, which papers were named) are re-stated in
    // `turn_context` precisely so that dropping the beginning is safe.
    let thread = match app
        .store()
        .conversations
        .messages_page(lib, &key, HISTORY_TURNS, None)
        .await
    {
        Ok(page) => page.messages,
        Err(e) => return run.fail(e.to_string()),
    };
    let mut history: Vec<yk_ai::ChatMessage> = thread.iter().map(to_chat).collect();

    // What this conversation is standing on, put in front of the model rather
    // than left for it to work out. A scoped conversation is scoped because
    // the user said so, and a paper they named with `@` is one they have
    // already chosen — making the model search for it again spends a step to
    // arrive somewhere it was already told about, and sometimes arrives at a
    // different paper.
    if let Some(context) = turn_context(&app, lib, &key, &thread).await {
        history.insert(0, yk_ai::ChatMessage::new("system", &context));
    }

    let progress = crate::runs::RunProgress {
        run: run.clone(),
        writers: crate::agent::ACTIONS.iter().filter(|a| a.writes()).map(|a| a.name()).collect(),
    };

    let turn = match agent.run_with(lib, history, &progress).await {
        Ok(turn) => turn,
        Err(e) => return run.fail(e.to_string()),
    };

    // Stored *before* the run is marked finished. The client drops the live
    // turn the moment it sees `running: false` and shows the stored message
    // instead; doing it the other way round leaves a frame with neither, which
    // reads as a flicker at the end of every answer.
    let state = run.snapshot();
    let stored = app
        .store()
        .conversations
        .append(
            lib,
            &key,
            MessageDraft {
                role: "assistant".into(),
                content: turn.reply.clone(),
                meta: Some(json!({
                    "model": agent.model(),
                    "truncated": turn.truncated,
                    "stopped": turn.stopped,
                    "trace": crate::runs::steps_json(&state),
                })),
                mentions: Vec::new(),
            },
        )
        .await;

    if let Err(e) = stored {
        tracing::warn!(error = %e, "could not store the agent's answer");
    }

    run.finish(turn.reply, turn.truncated, turn.stopped);
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
        Some(run) => crate::runs::state_json(&run.snapshot()),
        None => crate::runs::state_json(&crate::runs::RunState::default()),
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

async fn rename(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(patch): Json<yk_core::model::ConversationPatch>,
) -> ApiResult<Json<Conversation>> {
    Ok(Json(app.store().conversations.update(lib, &key(&k)?, patch).await?))
}

async fn remove(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let n = app.store().conversations.delete(lib, &key(&k)?).await?;
    Ok(Json(json!({ "deleted": n })))
}

/// How many messages a client gets when it does not say.
///
/// Enough to fill a tall window without scrolling, so opening a conversation
/// looks complete; the rest arrives when somebody scrolls back for it.
const MESSAGE_PAGE: u32 = 60;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageQuery {
    limit: Option<u32>,
    /// Everything older than this message id.
    before: Option<i64>,
}

async fn messages(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Query(q): Query<MessageQuery>,
) -> ApiResult<Json<yk_core::model::MessagePage>> {
    let page = app
        .store()
        .conversations
        .messages_page(lib, &key(&k)?, q.limit.unwrap_or(MESSAGE_PAGE), q.before)
        .await?;
    Ok(Json(page))
}

async fn append(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(draft): Json<MessageDraft>,
) -> ApiResult<Json<Message>> {
    Ok(Json(app.store().conversations.append(lib, &key(&k)?, draft).await?))
}
