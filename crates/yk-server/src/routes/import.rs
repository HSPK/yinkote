//! Importing another manager's library.
//!
//! Two steps on purpose: a preview says what would happen, and only a second,
//! explicit call does it. Merging one library into another is not something to
//! discover you have done.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::CollectionDraft;
use yk_core::Error;

use super::announce;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/import/zotero/preview", post(preview))
        .route("/libraries/:lib/import/zotero", post(run))
}

#[derive(Deserialize)]
struct Source {
    /// Path to `zotero.sqlite`, which is only ever opened read-only.
    path: String,
}

async fn preview(
    State(_app): State<App>,
    Json(body): Json<Source>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = std::path::PathBuf::from(&body.path);
    let seen = tokio::task::spawn_blocking(move || yk_import::zotero::preview(&path))
        .await
        .map_err(|e| Error::internal(e.to_string()))??;
    Ok(Json(json!(seen)))
}

/// Read a Zotero library and merge it into this one.
///
/// Zotero's keys are kept, so importing the same library twice updates the
/// items rather than duplicating them — which is the difference between an
/// import being repeatable and being a trap.
async fn run(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<Source>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = std::path::PathBuf::from(&body.path);
    let library = tokio::task::spawn_blocking(move || yk_import::zotero::read(&path))
        .await
        .map_err(|e| Error::internal(e.to_string()))??;

    // Collections first: an item's membership is meaningless until they exist.
    // Parents before children, so a nested collection has something to hang on.
    let mut ordered = library.collections.clone();
    ordered.sort_by_key(|c| c.parent.is_some());

    let mut collections = 0u64;
    for c in ordered {
        let draft = CollectionDraft {
            name: c.name,
            parent_key: c.parent,
            key: Some(c.key),
            ..Default::default()
        };
        // A collection that already exists is not a failure; it is the second
        // run of an import that is meant to be repeatable.
        if app.store().collections.create(lib, draft).await.is_ok() {
            collections += 1;
        }
    }

    let total = library.items.len();
    let mut added = 0u64;
    let mut updated = 0u64;
    let mut failed = 0u64;

    // In batches, so a large library does not hold the write lock for minutes.
    for chunk in library.items.chunks(200) {
        let keys: Vec<_> = chunk.iter().map(|d| d.key.clone()).collect();
        let results = app.store().items.create_many(lib, chunk.to_vec()).await?;

        for (result, (draft, key)) in results.into_iter().zip(chunk.iter().zip(keys)) {
            match result {
                Ok(item) => {
                    added += 1;
                    file_into_collections(&app, lib, &item.key, &library.membership).await;
                }
                // An item that is already here has not failed; it was imported
                // before. Bringing the newer version across is what makes a
                // repeat import worth running at all.
                Err(_) => match refresh(&app, lib, key.as_ref(), draft).await {
                    Ok(item) => {
                        updated += 1;
                        file_into_collections(&app, lib, &item.key, &library.membership).await;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "skipped an item during import");
                        failed += 1;
                    }
                },
            }
        }
    }

    let files = import_attachments(&app, lib, &library.attachments).await;
    let notes = import_notes(&app, lib, &library.notes).await;
    let annotations = import_annotations(&app, lib, &library).await;

    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: Vec::new(),
        version,
    })
    .await?;

    Ok(Json(json!({
        "items": added,
        // Kept apart from `added` so a second run reads as "nothing new" rather
        // than as a hundred failures, which is what it looked like before.
        "updated": updated,
        "collections": collections,
        "files": files,
        "notes": notes,
        "annotations": annotations,
        // Reported rather than hidden: an import that quietly dropped a tenth
        // of a library would be found out much later, by its absence.
        "failed": failed,
        "total": total,
        "version": version,
    })))
}

/// Create attachment records and copy across whatever bytes Zotero has.
///
/// A file Zotero only links to is recorded but not copied: it lives somewhere
/// else on the user's machine and is not ours to move. A file Zotero should
/// have but does not — a half-finished sync — is recorded too, because the
/// record is what says the item is missing its PDF.
async fn import_attachments(
    app: &App,
    lib: i64,
    attachments: &[yk_import::zotero::ImportedAttachment],
) -> u64 {
    let mut copied = 0;
    for attachment in attachments {
        let mut draft = yk_core::model::ItemDraft::new("attachment")
            .with_field("title", attachment.title.as_str())
            .with_field("filename", attachment.filename.as_str())
            .with_field("contentType", attachment.content_type.as_str())
            .with_field("linkMode", "imported_file");
        draft.key = Some(attachment.key.clone());
        draft.parent_key = Some(attachment.parent.clone());

        // Already present from an earlier import; the bytes are what matter now.
        let _ = app.store().items.create(lib, draft).await;

        let Some(source) = &attachment.source else { continue };
        match tokio::fs::read(source).await {
            Ok(bytes) => {
                if app.storage().put(&attachment.key, &attachment.filename, &bytes).await.is_ok() {
                    copied += 1;
                }
            }
            Err(e) => tracing::warn!(path = %source.display(), error = %e, "could not read a file"),
        }
    }
    copied
}

/// Bring the user's own notes across.
///
/// Kept as HTML, which is how both Zotero and this project store them, so a
/// note arrives looking the way its author left it rather than flattened.
async fn import_notes(app: &App, lib: i64, notes: &[yk_import::zotero::ImportedNote]) -> u64 {
    let mut imported = 0;
    for note in notes {
        let mut draft =
            yk_core::model::ItemDraft::new("note").with_field("note", note.html.as_str());
        // Without a title a note is a blank row in the library list. Zotero
        // keeps a one-line summary, so prefer theirs and fall back to the
        // note's own first line.
        let title = match note.title.trim() {
            "" => yk_core::text::note_title(&note.html, yk_core::text::NOTE_TITLE_CHARS),
            given => given.to_string(),
        };
        if !title.is_empty() {
            draft = draft.with_field("title", title.as_str());
        }
        draft.key = Some(note.key.clone());
        draft.parent_key = note.parent.clone();

        match app.store().items.create(lib, draft).await {
            Ok(_) => imported += 1,
            // Already here from an earlier import: update it, since the user
            // may well have kept writing in Zotero since.
            Err(_) => {
                let patch = serde_json::from_value(json!({ "fields": { "note": note.html } }));
                if let Ok(patch) = patch {
                    if app.store().items.update(lib, &note.key, patch, None).await.is_ok() {
                        imported += 1;
                    }
                }
            }
        }
    }
    imported
}

/// Bring across the highlights somebody made while reading.
///
/// An annotation hangs off the attachment it was drawn on, so one whose
/// attachment did not come across has nothing to hang on and is skipped rather
/// than filed against the paper — a highlight with no page is not a highlight.
///
/// The geometry is stored exactly as Zotero wrote it: PDF points, bottom-left
/// origin. The viewer converts it when it opens the page, because that is the
/// only place the page's size in points is known. See `yk_import::zotero`.
async fn import_annotations(
    app: &App,
    lib: i64,
    library: &yk_import::zotero::Imported,
) -> u64 {
    let files: std::collections::HashSet<&str> =
        library.attachments.iter().map(|a| a.key.as_str()).collect();

    let mut imported = 0;
    for a in &library.annotations {
        if !files.contains(a.parent.as_str()) {
            continue;
        }

        let fields = json!({
            "annotationType": a.kind,
            "annotationText": a.text,
            "annotationComment": a.comment,
            "annotationColor": a.colour,
            "annotationPage": a.page,
            "annotationPosition": a.position,
        });

        let mut draft = yk_core::model::ItemDraft::new("annotation");
        draft.key = Some(a.key.clone());
        draft.parent_key = Some(a.parent.clone());
        if let Some(map) = fields.as_object() {
            draft.fields = map.clone().into_iter().collect();
        }

        match app.store().items.create(lib, draft).await {
            Ok(_) => imported += 1,
            // Already here from an earlier import. The comment is the part a
            // user keeps editing, so a repeat run should carry it over.
            Err(_) => {
                let patch = serde_json::from_value(json!({ "fields": fields }));
                if let Ok(patch) = patch {
                    if app.store().items.update(lib, &a.key, patch, None).await.is_ok() {
                        imported += 1;
                    }
                }
            }
        }
    }
    imported
}

/// Bring an already-imported item up to date with its Zotero original.
async fn refresh(
    app: &App,
    lib: i64,
    key: Option<&yk_core::Key>,
    draft: &yk_core::model::ItemDraft,
) -> yk_core::Result<yk_core::model::Item> {
    let key = key.ok_or_else(|| Error::invalid("an item with no key cannot be matched"))?;
    let patch = serde_json::from_value(json!({
        "itemType": draft.item_type,
        "fields": draft.fields,
        "creators": draft.creators,
        "tags": draft.tags,
    }))
    .map_err(|e| Error::internal(e.to_string()))?;

    app.store().items.update(lib, key, patch, None).await
}

/// Put an item into every collection Zotero had it in.
async fn file_into_collections(
    app: &App,
    lib: i64,
    key: &yk_core::Key,
    membership: &std::collections::HashMap<String, Vec<yk_core::Key>>,
) {
    for collection in membership.get(key.as_str()).into_iter().flatten() {
        let _ = app
            .store()
            .items
            .add_to_collection(lib, collection, std::slice::from_ref(key))
            .await;
    }
}
