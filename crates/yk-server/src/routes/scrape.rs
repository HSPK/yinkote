//! Quick-add: paste anything, get a proper item.
//!
//! `/resolve` is a dry run that returns previews, so the UI can show the user
//! what it found before writing. `/quick-add` does the same work and commits,
//! skipping anything already in the library.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::{Item, ItemDraft, ItemTag};
use yk_core::plugin::hooks;
use yk_core::{Error, Key};
use yk_scrape::{Identifier, Resolution, SourceInfo};

use super::{announce, key, notify_plugins};
use crate::error::ApiResult;
use crate::state::App;

/// Resolving is a network round trip per identifier; cap the fan-out.
const MAX_RESOLVE: usize = 8;

pub fn router() -> Router<App> {
    Router::new()
        .route("/resolve", post(resolve))
        .route("/resolve/sources", get(sources))
        .route("/libraries/:lib/quick-add", post(quick_add))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveBody {
    text: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DetectedIdentifier {
    kind: String,
    value: String,
}

impl From<&Identifier> for DetectedIdentifier {
    fn from(id: &Identifier) -> Self {
        Self { kind: id.kind().to_string(), value: id.value().to_string() }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolveResponse {
    /// Everything recognised in the input, even if it could not be resolved,
    /// so the UI can explain what happened.
    identifiers: Vec<DetectedIdentifier>,
    resolutions: Vec<Resolution>,
    took_ms: u64,
}

async fn resolve(
    State(app): State<App>,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<ResolveResponse>> {
    let started = std::time::Instant::now();
    let identifiers = yk_scrape::detect(&body.text);
    let limit = body.limit.unwrap_or(3).clamp(1, MAX_RESOLVE);
    let resolutions = app.scrape().resolve_text(&body.text, limit).await;

    Ok(Json(ResolveResponse {
        identifiers: identifiers.iter().map(DetectedIdentifier::from).collect(),
        resolutions,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

async fn sources(State(app): State<App>) -> Json<Vec<SourceInfo>> {
    Json(app.scrape().sources())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuickAddBody {
    text: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// Add anyway even if an identical item already exists.
    #[serde(default)]
    allow_duplicates: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuickAddResponse {
    created: Vec<Item>,
    /// Already in the library; reported rather than silently re-added.
    duplicates: Vec<Duplicate>,
    /// Recognised but no source could resolve it.
    unresolved: Vec<DetectedIdentifier>,
    version: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Duplicate {
    identifier: String,
    existing_key: Key,
    title: String,
}

async fn quick_add(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<QuickAddBody>,
) -> ApiResult<Json<QuickAddResponse>> {
    if body.text.trim().is_empty() {
        return Err(Error::invalid("nothing to add").into());
    }
    let collection = body.collection.as_deref().map(key).transpose()?;
    let limit = body.limit.unwrap_or(3).clamp(1, MAX_RESOLVE);

    let detected = yk_scrape::detect(&body.text);
    let resolutions = app.scrape().resolve_text(&body.text, limit).await;
    if resolutions.is_empty() {
        return Err(Error::not_found("no metadata could be resolved for that input").into());
    }

    // Which identifiers did we recognise but fail to resolve?
    let resolved: Vec<&str> = resolutions.iter().map(|r| r.identifier.as_str()).collect();
    let unresolved: Vec<DetectedIdentifier> = detected
        .iter()
        .filter(|id| !resolved.contains(&id.value()))
        .map(DetectedIdentifier::from)
        .collect();

    let mut drafts: Vec<ItemDraft> = Vec::new();
    let mut duplicates = Vec::new();

    for resolution in resolutions {
        let mut draft = resolution.draft;
        // Provenance is worth keeping: it explains where a field came from.
        draft.fields.entry("extra".to_string()).or_insert_with(|| {
            json!(format!("Added via {} ({}:{})", resolution.source, resolution.kind, resolution.identifier))
        });
        for tag in &body.tags {
            draft.tags.push(ItemTag::manual(tag));
        }
        if let Some(c) = &collection {
            draft.collections.push(c.clone());
        }

        if !body.allow_duplicates {
            let fingerprint = draft_fingerprint(&draft);
            let existing = app.store().items.find_by_fingerprint(lib, &[fingerprint]).await?;
            if let Some(found) = existing.first() {
                duplicates.push(Duplicate {
                    identifier: resolution.identifier,
                    existing_key: found.key.clone(),
                    title: found.title().to_string(),
                });
                continue;
            }
        }
        drafts.push(draft);
    }

    if drafts.is_empty() {
        return Ok(Json(QuickAddResponse {
            created: Vec::new(),
            duplicates,
            unresolved,
            version: app.store().libraries.version(lib).await?,
        }));
    }

    let results = app.store().items.create_many(lib, drafts).await?;
    let created: Vec<Item> = results.into_iter().filter_map(Result::ok).collect();
    let keys: Vec<Key> = created.iter().map(|i| i.key.clone()).collect();

    let version = announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: keys.clone(),
        version,
    })
    .await?;
    notify_plugins(&app, hooks::ITEM_CREATED, json!({ "libraryId": lib, "items": created }));

    Ok(Json(QuickAddResponse { created, duplicates, unresolved, version }))
}

/// Same rule the store uses, applied to a draft that has no key yet.
fn draft_fingerprint(draft: &ItemDraft) -> String {
    draft.clone().into_item(Key::generate(), 0, 0).fingerprint()
}
