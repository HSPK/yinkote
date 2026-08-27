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
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_cite::export::Export;
use yk_cite::Format;
use yk_core::Error;

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/citation-styles", get(list_styles))
        .route("/libraries/:lib/citations", post(render))
        .route("/libraries/:lib/export", post(export))
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

    // One query, in the caller's order. A numeric style numbers by first
    // appearance in the text and the server cannot see the text, so the order
    // that arrives is the only order there is.
    let mut keys = Vec::with_capacity(body.keys.len());
    for k in &body.keys {
        keys.push(key(k)?);
    }
    let items = super::items_in_order(&app, lib, &keys).await?;

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

#[derive(Deserialize)]
struct ExportBody {
    #[serde(rename = "itemKeys", alias = "keys")]
    item_keys: Vec<String>,
    format: String,
}

/// Hand a set of items to another program.
///
/// Sent as a file rather than as JSON wrapping a string: the browser saves it
/// straight to disk, and what somebody does with an export is drop it next to a
/// `.tex` file. Every format is text, so the download is the whole answer.
async fn export(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<ExportBody>,
) -> ApiResult<Response> {
    let format = Export::parse(&body.format).ok_or_else(|| {
        Error::invalid(format!(
            "no such export format: {} (bibtex, ris, csljson)",
            body.format
        ))
    })?;

    // Order is the caller's, as with rendering.
    let mut keys = Vec::with_capacity(body.item_keys.len());
    for k in &body.item_keys {
        keys.push(key(k)?);
    }
    let items = super::items_in_order(&app, lib, &keys).await?;
    let text = yk_cite::export::export(&items, format);

    let mut headers = HeaderMap::new();
    if let Ok(v) = format!("{}; charset=utf-8", format.content_type()).parse() {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) =
        format!("attachment; filename=\"yinkote.{}\"", format.extension()).parse()
    {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok((headers, text).into_response())
}
