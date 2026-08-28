//! System endpoints: health, schema, libraries, settings, maintenance and the
//! WebSocket event stream.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::error::RecvError;
use yk_core::model::Library;
use yk_core::schema::{schema, Schema};

use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/ping", get(ping))
        .route("/schema", get(get_schema))
        .route("/stats", get(stats))
        .route("/libraries", get(libraries))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/maintenance/reindex/:lib", post(reindex))
        .route("/maintenance/optimize", post(optimize))
        .route("/events", get(events))
}

async fn ping(State(app): State<App>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "yinkote",
        "version": env!("CARGO_PKG_VERSION"),
        "apiVersion": 1,
        "pluginApiVersion": yk_core::plugin::PLUGIN_API_VERSION,
        "uptimeSecs": app.uptime_secs(),
        "defaultLibrary": app.services.default_library,
        // Surfaced so the Settings page can tell the user where their data
        // actually lives — a local-first app owes them that.
        "dataDir": app.config().data_dir().display().to_string(),
        "pluginDirs": app.config()
            .all_plugin_dirs()
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>(),
        "bind": app.config().bind_addr(),
    }))
}

/// The item-type schema, shared verbatim with the frontend so both sides agree
/// on fields without duplicating definitions.
async fn get_schema() -> Json<&'static Schema> {
    Json(schema())
}

async fn libraries(State(app): State<App>) -> ApiResult<Json<Vec<Library>>> {
    Ok(Json(app.store().libraries.list().await?))
}

async fn stats(State(app): State<App>) -> ApiResult<Json<Value>> {
    let lib = app.services.default_library;
    let filter = yk_core::query::ItemFilter { library_id: lib, ..Default::default() };
    let trashed = yk_core::query::ItemFilter {
        library_id: lib,
        trash: yk_core::query::TrashScope::Only,
        ..Default::default()
    };
    // Six independent questions, so the answer takes as long as the slowest
    // rather than as long as all of them. Written out in one `join!` instead of
    // awaited in turn: nothing here reads anything else here, and the sum was
    // most of what the workbench waited for on load.
    let (items, trashed, collections, tags, search, version) = tokio::join!(
        app.store().items.count(&filter),
        app.store().items.count(&trashed),
        app.store().collections.count(lib),
        app.store().tags.count(lib),
        app.search().stats(),
        app.store().libraries.version(lib),
    );

    Ok(Json(json!({
        "items": items?,
        "trashed": trashed?,
        "collections": collections?,
        "tags": tags?,
        "search": search?,
        "plugins": app.plugins.list().await.len(),
        "version": version?,
        "uptimeSecs": app.uptime_secs(),
        "wsClients": app.events().subscriber_count(),
    })))
}

async fn get_settings(State(app): State<App>) -> ApiResult<Json<Value>> {
    let entries = app.store().settings.list("ui.").await?;
    Ok(Json(Value::Object(entries.into_iter().collect())))
}

#[derive(Deserialize)]
struct SettingsBody(serde_json::Map<String, Value>);

async fn put_settings(
    State(app): State<App>,
    Json(body): Json<SettingsBody>,
) -> ApiResult<Json<Value>> {
    for (k, v) in body.0 {
        // Namespaced so plugin settings can never be written from here.
        let key = if k.starts_with("ui.") { k } else { format!("ui.{k}") };
        app.store().settings.set(&key, &v).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Rebuild the search index.
///
/// Started rather than awaited: half a minute on a hundred thousand items, and
/// for most of it the interface had nothing to say. It reports no count —
/// rebuilding is two coarse passes over the library, not a countable sequence —
/// so the task's `total` stays zero, which the interface reads as "spinner,
/// not bar". Claiming a percentage nobody can compute would be worse.
async fn reindex(State(app): State<App>, Path(lib): Path<i64>) -> Json<Value> {
    let task = app.tasks().start("reindex", "Rebuilding the search index");
    let running = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        match running.search().reindex(lib).await {
            Ok(n) => running.tasks().finish(&handle, json!({ "reindexed": n })),
            Err(e) => running.tasks().fail(&handle, e),
        }
    });
    Json(json!({ "task": task.snapshot() }))
}

async fn optimize(State(app): State<App>) -> ApiResult<Json<Value>> {
    app.store().db().maintenance().await?;
    Ok(Json(json!({ "ok": true })))
}

/// Live change feed. Clients apply deltas instead of re-fetching everything.
async fn events(State(app): State<App>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| stream_events(app, socket))
}

async fn stream_events(app: App, mut socket: WebSocket) {
    let mut rx = app.events().subscribe();
    let hello = json!({
        "type": "hello",
        "version": env!("CARGO_PKG_VERSION"),
        "defaultLibrary": app.services.default_library,
    });
    if socket.send(Message::Text(hello.to_string())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(e) => {
                    let Ok(text) = serde_json::to_string(&e) else { continue };
                    if socket.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
                // A slow client misses events rather than blocking writers;
                // tell it so it can resynchronise with `?since=`.
                Err(RecvError::Lagged(n)) => {
                    let warn = json!({ "type": "lagged", "missed": n });
                    if socket.send(Message::Text(warn.to_string())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Ping(p))) => {
                    if socket.send(Message::Pong(p)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(_)) => {}
                _ => break,
            },
        }
    }
}
