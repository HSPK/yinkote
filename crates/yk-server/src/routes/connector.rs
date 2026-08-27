//! Speaking the Zotero connector's protocol.
//!
//! The browser extension that saves a paper from a publisher's page is by far
//! the most-used part of a reference manager, and it is also the hardest part
//! to write: it carries hundreds of site-specific translators, maintained for
//! twenty years, that know where the metadata hides on each of them.
//!
//! Rewriting that is not a good use of anybody's life. The extension is already
//! installed in the user's browser, it is open source, and it talks to a plain
//! local HTTP endpoint. So this speaks its protocol instead. The translators
//! stay where they are and keep being maintained by the people who know those
//! sites; this end only has to receive what they produce.
//!
//! Two honest limitations, both stated rather than papered over:
//!
//! **It is off by default.** The connector expects port 23119, which belongs to
//! Zotero. Taking it silently would break a running Zotero — the one thing a
//! tool offering to replace it must not do. `--connector-port 23119` is an
//! explicit choice by somebody who knows they are not running both.
//!
//! **It is not authenticated.** The extension has no way to hold an API key, so
//! these routes sit outside the API guard. They are loopback-only, and they
//! accept exactly the shapes below and nothing else.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use yk_core::event::DomainEvent;
use yk_core::model::{Creator, ItemDraft, ItemTag};

use super::announce;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/connector/ping", get(ping_text).post(ping))
        .route("/connector/getSelectedCollection", post(selected_collection))
        .route("/connector/saveItems", post(save_items))
        .route("/connector/saveSnapshot", post(save_snapshot))
        .route("/connector/updateSession", post(update_session))
}

/// Keys the connector sends that describe *structure* rather than a field.
///
/// Everything else is passed through, because both schemas name their fields
/// the same way — this project's item types were drawn from Zotero's.
const STRUCTURAL: &[&str] =
    &["itemType", "creators", "tags", "notes", "attachments", "seeAlso", "id", "uri", "key"];

/// A browser opening the address sees words rather than a blank page.
async fn ping_text() -> impl IntoResponse {
    (StatusCode::OK, "Yinkote is running, and speaking the Zotero connector protocol.")
}

/// What the extension asks before offering to save.
async fn ping(State(app): State<App>) -> Json<Value> {
    Json(json!({
        "prefs": {
            // Snapshots are whole-page HTML archives. This project stores files
            // it can open again; a snapshot it cannot render would be a
            // directory of orphaned markup.
            "automaticSnapshots": false,
            // The PDF is the point of saving a paper, so yes.
            "downloadAssociatedFiles": true,
            "reportActiveURL": false,
            "translatorsHash": Value::Null,
        },
        "libraryID": app.services.default_library,
    }))
}

/// Where a save would land. Always the library root for now.
async fn selected_collection(State(app): State<App>) -> Json<Value> {
    Json(json!({
        "libraryID": app.services.default_library,
        "libraryName": "My Library",
        "libraryEditable": true,
        "editable": true,
        "id": Value::Null,
        "name": "My Library",
    }))
}

#[derive(Deserialize)]
struct SaveItems {
    #[serde(default)]
    items: Vec<Value>,
}

/// Take what a translator found on the page.
///
/// The reply is the connector's own shape — it shows the saved titles in its
/// popup — so it is built from what was actually stored rather than echoed back
/// from the request. If a field was dropped on the way in, the popup says so.
async fn save_items(
    State(app): State<App>,
    Json(body): Json<SaveItems>,
) -> impl IntoResponse {
    let lib = app.services.default_library;
    let mut saved = Vec::new();

    for value in &body.items {
        let Some(object) = value.as_object() else { continue };
        let draft = to_draft(object);

        let item = match app.store().items.create(lib, draft).await {
            Ok(item) => item,
            Err(e) => {
                tracing::warn!(error = %e, "connector item rejected");
                continue;
            }
        };

        for note in notes(object) {
            let mut child = ItemDraft::new("note")
                .with_field("note", note.as_str())
                .with_field(
                    "title",
                    yk_core::text::note_title(note.as_str(), yk_core::text::NOTE_TITLE_CHARS)
                        .as_str(),
                );
            child.parent_key = Some(item.key.clone());
            let _ = app.store().items.create(lib, child).await;
        }

        // Downloading happens after the item exists, so a paper is saved even
        // when its PDF is behind a paywall the browser could see through and
        // this cannot.
        for (title, url) in attachments(object) {
            if let Err(e) = super::files::attach_url(&app, lib, &item.key, &url, &title).await {
                tracing::info!(error = %e, url, "could not fetch an attachment");
            }
        }

        saved.push(json!({
            "id": item.key,
            "key": item.key,
            "itemType": item.item_type,
            "title": item.title(),
        }));
    }

    let _ = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: Vec::new(),
        version,
    })
    .await;

    // The connector treats anything but 201 as a failure worth retrying.
    (StatusCode::CREATED, Json(json!({ "items": saved })))
}

#[derive(Deserialize)]
struct Snapshot {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

/// A page with no translator behind it: keep the address and the title.
///
/// Not the HTML. A snapshot this cannot render again is a directory of
/// orphaned markup pretending to be an archive.
async fn save_snapshot(
    State(app): State<App>,
    Json(body): Json<Snapshot>,
) -> impl IntoResponse {
    let lib = app.services.default_library;
    let url = body.url.unwrap_or_default();
    let draft = ItemDraft::new("webpage")
        .with_field("title", body.title.unwrap_or_else(|| url.clone()).as_str())
        .with_field("url", url.as_str());

    match app.store().items.create(lib, draft).await {
        Ok(item) => {
            let _ = announce(&app, lib, |version| DomainEvent::ItemsChanged {
                library_id: lib,
                keys: vec![item.key.clone()],
                version,
            })
            .await;
            (StatusCode::CREATED, Json(json!({ "items": [{ "key": item.key }] })))
        }
        Err(e) => {
            tracing::warn!(error = %e, "connector snapshot rejected");
            (StatusCode::CREATED, Json(json!({ "items": [] })))
        }
    }
}

/// The connector tells us where the user filed the save, afterwards.
///
/// Accepted and ignored: everything lands in the library root, and saying so
/// with a 200 is better than a 404 the extension reports as a failed save.
async fn update_session() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({})))
}

/// Turn one translated item into a draft.
fn to_draft(object: &Map<String, Value>) -> ItemDraft {
    let item_type = object.get("itemType").and_then(Value::as_str).unwrap_or("webpage");
    let mut draft = ItemDraft::new(item_type);

    for (name, value) in object {
        if STRUCTURAL.contains(&name.as_str()) || value.is_null() {
            continue;
        }
        // Only scalars: a translator occasionally emits a nested object for a
        // field this has no column for, and storing it would produce a field
        // nothing can display or search.
        if value.is_string() || value.is_number() || value.is_boolean() {
            draft.fields.insert(name.clone(), value.clone());
        }
    }

    draft.creators = object
        .get("creators")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(creator).collect())
        .unwrap_or_default();

    draft.tags = object
        .get("tags")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(tag).collect())
        .unwrap_or_default();

    draft
}

/// Connectors send `{firstName, lastName}` or `{name}`, and sometimes a bare
/// string. A single-field name is kept whole — splitting it would invent a
/// surname for an institution and mangle most CJK names.
fn creator(value: &Value) -> Option<Creator> {
    if let Some(name) = value.as_str() {
        return Some(Creator {
            creator_type: "author".into(),
            name: Some(name.to_string()),
            ..Default::default()
        });
    }

    let object = value.as_object()?;
    let creator_type = object
        .get("creatorType")
        .and_then(Value::as_str)
        .unwrap_or("author")
        .to_string();

    let first = object.get("firstName").and_then(Value::as_str);
    let last = object.get("lastName").and_then(Value::as_str);
    if first.is_some() || last.is_some() {
        return Some(Creator {
            creator_type,
            first_name: first.map(str::to_string),
            last_name: last.map(str::to_string),
            ..Default::default()
        });
    }

    let name = object.get("name").and_then(Value::as_str)?;
    Some(Creator { creator_type, name: Some(name.to_string()), ..Default::default() })
}

fn tag(value: &Value) -> Option<ItemTag> {
    let name = match value {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("tag").and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    if name.trim().is_empty() {
        return None;
    }
    // Type 1 is automatic: these came from a translator, not from the user, and
    // a library where the two are indistinguishable cannot be tidied.
    Some(ItemTag { tag: name, r#type: 1 })
}

fn notes(object: &Map<String, Value>) -> Vec<String> {
    object
        .get("notes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|n| match n {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(o) => o.get("note").and_then(Value::as_str).map(str::to_string),
                    _ => None,
                })
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The files a translator found, as `(title, url)`.
///
/// Snapshots are skipped for the same reason `saveSnapshot` keeps only the
/// address: an archive that cannot be opened again is not an archive.
fn attachments(object: &Map<String, Value>) -> Vec<(String, String)> {
    object
        .get("attachments")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let o = a.as_object()?;
                    let url = o.get("url").and_then(Value::as_str)?.to_string();
                    let mime = o.get("mimeType").and_then(Value::as_str).unwrap_or("");
                    if mime == "text/html" || o.get("snapshot").and_then(Value::as_bool) == Some(true)
                    {
                        return None;
                    }
                    let title = o
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Full Text PDF")
                        .to_string();
                    Some((title, url))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
