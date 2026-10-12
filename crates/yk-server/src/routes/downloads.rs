//! The download queue's surface.
//!
//! Adding is deliberately plural and deliberately cheap: it records the
//! intention and returns. Everything slow happens in the worker, which is what
//! makes "paste twenty links" a reasonable thing to do.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use yk_core::Error;
use yk_store::DownloadDraft;

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn api() -> crate::operations::Api {
    crate::api_routes! {
        GET "/libraries/:lib/downloads" => list, "list_downloads", Read, "List pending, failed and completed downloads.";
        POST "/libraries/:lib/downloads" => enqueue, "queue_downloads", Write, "Queue attachment downloads for an item.";
        POST "/libraries/:lib/downloads/retry" => retry, "retry_downloads", Write, "Retry selected failed downloads.";
        POST "/libraries/:lib/downloads/remove" => remove, "remove_downloads", Write, "Remove download queue entries, not library files.";
        POST "/libraries/:lib/downloads/clear" => clear, "clear_finished_downloads", Write, "Clear finished download entries.";
    }
}

/// The queue, unfinished rows first.
const PAGE: u32 = 200;

async fn list(
    State(app): State<App>,
    Path(lib): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let rows = app.store().downloads.list(lib, PAGE).await?;
    Ok(Json(json!({
        "downloads": rows,
        // Counted here rather than in the client so every surface showing a
        // badge agrees about what it means.
        "waiting": rows.iter().filter(|d| d.state == "waiting").count(),
        "failed": rows.iter().filter(|d| d.state == "failed").count(),
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Enqueue {
    /// The item the files belong to.
    #[serde(rename = "itemKey")]
    item_key: String,
    /// One or more addresses. Plural because a paper often has a PDF, a
    /// supplement and a dataset, and asking three times is three chances to
    /// give up.
    urls: Vec<String>,
    #[serde(default)]
    title: Option<String>,
}

async fn enqueue(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<Enqueue>,
) -> ApiResult<Json<serde_json::Value>> {
    let item = key(&body.item_key)?;
    // Checked now rather than in the worker: an item key that does not exist is
    // a mistake by the caller, and finding out at download time would report it
    // as a failed download instead.
    let known = app.store().items.get(lib, &item).await?;

    let title = body.title.unwrap_or_else(|| known.title().to_string());
    let drafts: Vec<DownloadDraft> = body
        .urls
        .iter()
        .map(|url| DownloadDraft {
            item_key: item.to_string(),
            url: url.trim().to_string(),
            title: title.clone(),
        })
        .collect();

    if drafts.is_empty() {
        return Err(Error::invalid("no addresses were given").into());
    }

    let added = app.store().downloads.enqueue(lib, drafts).await?;
    Ok(Json(json!({ "queued": added })))
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Ids {
    ids: Vec<i64>,
}

async fn retry(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<Ids>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "retrying": app.store().downloads.retry(lib, &body.ids).await? })))
}

async fn remove(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<Ids>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "removed": app.store().downloads.remove(lib, &body.ids).await? })))
}

/// Forget what finished, keeping what still needs a decision.
async fn clear(
    State(app): State<App>,
    Path(lib): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "cleared": app.store().downloads.clear_finished(lib).await? })))
}
