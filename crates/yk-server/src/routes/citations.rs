//! Rendering citations and bibliographies.
//!
//! The endpoint takes keys rather than items so that the client never has to
//! send back what the server already has, and so that a Word plugin — which
//! will know a key and nothing else — can ask the same question the workbench
//! asks.
//!
//! Order is the caller's. A numeric style numbers entries by first appearance
//! in the text, and the server cannot see the text; sorting here would silently
//! renumber somebody's paper.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_cite::Format;
use yk_core::Error;

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/citation-styles", get(list_styles))
        .route("/libraries/:lib/citations", post(render))
}

async fn list_styles() -> Json<serde_json::Value> {
    let styles: Vec<_> = yk_cite::STYLES
        .iter()
        .map(|s| json!({ "id": s.id, "name": s.name, "numeric": s.numeric }))
        .collect();
    Json(json!(styles))
}

#[derive(Deserialize)]
struct RenderBody {
    keys: Vec<String>,
    style: String,
    /// `text` or `html`. Text is what a clipboard wants; HTML keeps the
    /// italics a word processor needs.
    #[serde(default)]
    format: Option<String>,
}

async fn render(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<RenderBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let style = yk_cite::find(&body.style)
        .ok_or_else(|| Error::invalid(format!("no such citation style: {}", body.style)))?;

    let format =
        if body.format.as_deref() == Some("html") { Format::Html } else { Format::Text };

    let mut items = Vec::with_capacity(body.keys.len());
    for k in &body.keys {
        items.push(app.store().items.get(lib, &key(k)?).await?);
    }

    let entries = yk_cite::bibliography(&items, style, format);
    let citations: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(i, item)| yk_cite::citation(item, style, i + 1))
        .collect();

    Ok(Json(json!({
        "style": style.id,
        "citations": citations,
        "bibliography": entries,
    })))
}
