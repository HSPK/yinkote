//! Browsing the library's files.
//!
//! The storage directory is something people open in a file manager, sync
//! between machines and grep. A view of it belongs in the workbench for the
//! same reason the collection browser does: it is a thing you look through, not
//! a thing you dismiss.
//!
//! Renaming is offered in two halves. `preview` says exactly what every file
//! would be called and changes nothing; `rename` does it. A batch rename nobody
//! can look at first is one nobody should run.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::model::{Item, ItemPatch};
use yk_core::{Error, Key};

use crate::error::ApiResult;
use crate::naming::{filename_for, DEFAULT_TEMPLATE};
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/files", get(list))
        .route("/libraries/:lib/files/preview", post(preview))
        .route("/libraries/:lib/files/rename", post(rename))
}

/// How many files one page holds.
///
/// A library's attachments are counted in thousands, and a list that long is
/// scrolled rather than read; the page is what makes it answerable in one
/// query rather than one per file.
const PAGE: u32 = 500;

#[derive(Deserialize)]
struct Paging {
    #[serde(default)]
    offset: u32,
}

async fn list(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(paging): Query<Paging>,
) -> ApiResult<Json<serde_json::Value>> {
    let page = app.store().items.attachments(lib, PAGE, paging.offset).await?;

    // Every size in one pass before building the response: a `stat` per row,
    // awaited in turn, was thirty times the cost of the query that found them.
    let named: Vec<(Key, String)> = page
        .items
        .iter()
        .map(|(a, _)| (a.key.clone(), a.field("filename").unwrap_or_default().to_string()))
        .collect();
    let sizes = app.storage().sizes(&named).await;

    let mut files = Vec::with_capacity(page.items.len());
    for (i, (attachment, parent)) in page.items.iter().enumerate() {
        let filename = attachment.field("filename").unwrap_or_default();
        files.push(json!({
            "key": attachment.key,
            "parentKey": parent.as_ref().map(|p| p.key.clone()),
            "parentTitle": parent.as_ref().map(|p| p.title().to_string()).unwrap_or_default(),
            "filename": filename,
            "contentType": attachment.field("contentType").unwrap_or_default(),
            // Where it came from. Kept on the attachment since the day it was
            // fetched; surfaced here because "where did this PDF come from" is
            // the question a file browser is opened to answer.
            "url": attachment.field("url").unwrap_or_default(),
            // From disk rather than from the record: a file the database
            // believes in and the disk does not is exactly what this view is
            // for finding.
            "bytes": sizes.get(i).copied().unwrap_or(0),
        }));
    }

    Ok(Json(json!({ "files": files, "total": page.total, "offset": paging.offset })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Rename {
    /// A template such as `{author} {year} - {title}`.
    template: Option<String>,
    /// Only these attachments, or all of them when empty.
    keys: Vec<String>,
}

/// How many example renames to send.
///
/// A preview answers two questions — how many files change, and does the
/// pattern look right — and the second needs a handful of examples, not all of
/// them. Sending every row made a 3.7 MB response for a panel that shows eight
/// lines, measured against thirty thousand attachments.
const SAMPLE: usize = 50;

/// What renaming would do, without doing any of it.
async fn preview(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<Rename>,
) -> ApiResult<Json<serde_json::Value>> {
    let template = template_of(&app, body.template.clone()).await;
    let planned = plan(&app, lib, &body, &template).await?;

    Ok(Json(json!({
        "template": template,
        // The number is the answer; the rows are the evidence.
        "total": planned.len(),
        "changes": planned
            .iter()
            .take(SAMPLE)
            .map(|(attachment, from, to)| json!({
                "key": attachment.key,
                "from": from,
                "to": to,
            }))
            .collect::<Vec<_>>(),
    })))
}

async fn rename(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<Rename>,
) -> ApiResult<Json<serde_json::Value>> {
    let template = template_of(&app, body.template.clone()).await;
    let planned = plan(&app, lib, &body, &template).await?;

    let mut failed = 0u64;
    let mut moved: Vec<(Key, ItemPatch)> = Vec::with_capacity(planned.len());

    for (attachment, from, to) in planned {
        // The bytes move first. If the record were updated first and the move
        // then failed, the library would point at a file that does not exist —
        // whereas a moved file with a stale record is still findable.
        match app.storage().rename(&attachment.key, &from, &to).await {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(error = %e, from, to, "could not rename a file");
                failed += 1;
                continue;
            }
        }

        match serde_json::from_value(json!({ "fields": { "filename": to } })) {
            Ok(patch) => moved.push((attachment.key, patch)),
            Err(_) => failed += 1,
        }
    }

    // Every record in one write. Renaming used to open a transaction per file
    // and bump the library version each time: two minutes and thirty thousand
    // versions for a library this size, when what happened was one rename.
    let results = app.store().items.update_many(lib, moved).await?;
    let mut renamed = 0u64;
    for result in results {
        match result {
            Ok(_) => renamed += 1,
            Err(e) => {
                tracing::warn!(error = %e, "renamed the file but not the record");
                failed += 1;
            }
        }
    }

    Ok(Json(json!({ "renamed": renamed, "failed": failed })))
}

/// The template to use: the caller's, else the saved one, else the default.
async fn template_of(app: &App, given: Option<String>) -> String {
    if let Some(template) = given.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        return template;
    }
    app.store()
        .settings
        .get("ui.fileTemplate")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATE.to_string())
}

/// Work out every rename, skipping the ones that would change nothing.
///
/// A file already correctly named must not be "renamed": it would cost a write,
/// and a report of nine hundred renames when nothing moved is a report nobody
/// can act on.
async fn plan(
    app: &App,
    lib: i64,
    body: &Rename,
    template: &str,
) -> Result<Vec<(Item, String, String)>, Error> {
    let chosen: Vec<Key> =
        body.keys.iter().filter_map(|k| Key::parse(k).ok()).collect();

    let page = app.store().items.attachments(lib, u32::MAX, 0).await?;
    let mut out = Vec::new();

    for (attachment, parent) in page.items {
        if !chosen.is_empty() && !chosen.contains(&attachment.key) {
            continue;
        }
        let Some(parent) = parent else { continue };

        let from = attachment.field("filename").unwrap_or_default().to_string();
        if from.is_empty() {
            continue;
        }
        let to = filename_for(template, &parent, &from);
        if to == from || to.is_empty() {
            continue;
        }
        out.push((attachment, from, to));
    }

    Ok(out)
}
