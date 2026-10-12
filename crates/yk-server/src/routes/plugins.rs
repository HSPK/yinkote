//! Plugin management surface.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use yk_core::event::DomainEvent;
use yk_core::plugin::{hooks, Contributions, HookEvent, HookOutcome, PluginStatus};

use crate::error::ApiResult;
use crate::state::App;

pub fn api() -> crate::operations::Api {
    crate::api_routes! {
        GET "/plugins" => list, "list_plugins", Read, "Read plugin status.";
        POST "/plugins/reload" => reload, "reload_plugins", Dangerous, "Reload executable plugins.";
        GET "/plugins/contributions" => contributions, "plugin_contributions", Read, "Read plugin contributions.";
        POST "/plugins/dispatch" => dispatch, "dispatch_plugin_hook", Dangerous, "Dispatch a lifecycle hook.";
        GET "/plugins/:id" => get_one, "get_plugin", Read, "Read one plugin's status.";
        POST "/plugins/:id/enabled" => set_enabled, "set_plugin_enabled", Dangerous, "Enable or disable a plugin.";
        POST "/plugins/:id/call" => call, "call_plugin", Dangerous, "Call an arbitrary plugin method.";
    }
}

async fn list(State(app): State<App>) -> Json<Vec<PluginStatus>> {
    Json(app.plugins.list().await)
}

async fn get_one(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Json<PluginStatus>> {
    Ok(Json(app.plugins.get(&id).await?))
}

#[derive(Deserialize, schemars::JsonSchema)]
struct EnabledBody {
    enabled: bool,
}

async fn set_enabled(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<EnabledBody>,
) -> ApiResult<Json<PluginStatus>> {
    let status = app.plugins.set_enabled(&id, body.enabled).await?;
    // A disabled plugin's badges must stop appearing, and an enabled one's must
    // start; either way the cached answers no longer describe reality.
    app.badges.clear().await;
    persist_disabled(&app).await;
    app.events().publish(DomainEvent::PluginsChanged);
    Ok(Json(status))
}

async fn reload(State(app): State<App>) -> ApiResult<Json<Vec<PluginStatus>>> {
    app.plugins.reload().await?;
    app.badges.clear().await;
    app.events().publish(DomainEvent::PluginsChanged);
    Ok(Json(app.plugins.list().await))
}

async fn contributions(State(app): State<App>) -> Json<Contributions> {
    Json(app.plugins.contributions().await)
}

#[derive(Deserialize, schemars::JsonSchema)]
struct CallBody {
    method: String,
    #[serde(default)]
    params: Value,
}

async fn call(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<CallBody>,
) -> ApiResult<Json<Value>> {
    Ok(Json(app.plugins.call(&id, &body.method, body.params).await?))
}

#[derive(Deserialize, schemars::JsonSchema)]
struct DispatchBody {
    name: String,
    #[serde(default)]
    payload: Value,
}

/// Manual hook dispatch — useful for plugin development and for actions that
/// the UI triggers explicitly.
async fn dispatch(
    State(app): State<App>,
    Json(body): Json<DispatchBody>,
) -> ApiResult<Json<Vec<HookOutcome>>> {
    if !hooks::ALL.contains(&body.name.as_str()) {
        return Err(yk_core::Error::invalid(format!("unknown hook '{}'", body.name)).into());
    }
    Ok(Json(app.plugins.dispatch(HookEvent::new(body.name, body.payload)).await))
}

/// Remember which plugins the user switched off so the choice survives restart.
async fn persist_disabled(app: &App) {
    let disabled: Vec<String> = app
        .plugins
        .list()
        .await
        .into_iter()
        .filter(|p| matches!(p.state, yk_core::plugin::PluginState::Disabled))
        .map(|p| p.manifest.id)
        .collect();
    let _ = app.store().settings.set("plugins.disabled", &json!(disabled)).await;
}
