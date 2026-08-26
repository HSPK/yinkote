//! Attachment files: storing, fetching from the web, and serving.
//!
//! Fetching is what turns a reference into a readable paper, so it gets its own
//! endpoint rather than being something the user does by hand: give it an item
//! and it works out where the PDF is from the metadata already collected.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::ItemDraft;
use yk_core::{Error, Key, Result};

use super::{announce, key};
use crate::error::ApiResult;
use crate::state::App;
use crate::storage::{filename_from_url, MAX_BYTES};

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/files/:key", get(download).put(upload))
        .route("/libraries/:lib/items/:key/fetch", post(fetch))
}

/// Serve an attachment's bytes.
///
/// Inline rather than as a download: the point is to read it in the workbench,
/// and a browser that saves the file instead of showing it has missed it.
async fn download(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Response> {
    let key = key(&k)?;
    let item = app.store().items.get(lib, &key).await?;
    let filename = attachment_filename(&item)?;

    let bytes = app.storage().get(&key, &filename).await?;
    let mime = item.field("contentType").unwrap_or("application/octet-stream");

    let mut headers = HeaderMap::new();
    if let Ok(v) = mime.parse() {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = format!("inline; filename=\"{filename}\"").parse() {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok((headers, Body::from(bytes)).into_response())
}

/// Replace an attachment's bytes.
async fn upload(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    body: axum::body::Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let item = app.store().items.get(lib, &key).await?;
    let filename = attachment_filename(&item)?;

    app.storage().put(&key, &filename, &body).await?;
    announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![key.clone()],
        version,
    })
    .await?;
    Ok(Json(json!({ "key": key.as_str(), "bytes": body.len() })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FetchBody {
    /// Where to download from. Worked out from the item when absent.
    url: Option<String>,
}

/// Download the item's PDF and attach it.
///
/// Returns the attachment, so the caller can open it immediately.
async fn fetch(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<FetchBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let parent = key(&k)?;
    let item = app.store().items.get(lib, &parent).await?;

    let url = match body.url {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => pdf_url_for(&item).ok_or_else(|| {
            Error::invalid("no PDF address could be worked out; supply a url")
        })?,
    };

    let (bytes, content_type) = download_file(&url).await?;
    let filename = filename_from_url(&url, content_type.as_deref());

    // One attachment per source URL: fetching twice must not litter the item
    // with duplicates, which is easy to do by double-clicking.
    let existing = app
        .store()
        .items
        .children(lib, &parent)
        .await?
        .into_iter()
        .find(|c| c.item_type == "attachment" && c.field("url") == Some(url.as_str()));

    let attachment = match existing {
        Some(found) => {
            app.storage().put(&found.key, &filename, &bytes).await?;
            found
        }
        None => {
            let mut draft = ItemDraft::new("attachment")
                .with_field("title", item.title())
                .with_field("filename", filename.as_str())
                .with_field("contentType", content_type.as_deref().unwrap_or("application/pdf"))
                .with_field("linkMode", "imported_url")
                .with_field("url", url.as_str());
            draft.parent_key = Some(parent.clone());
            let created = app.store().items.create(lib, draft).await?;
            app.storage().put(&created.key, &filename, &bytes).await?;
            created
        }
    };

    announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![parent.clone(), attachment.key.clone()],
        version,
    })
    .await?;
    Ok(Json(json!({
        "attachment": attachment,
        "bytes": bytes.len(),
        "url": url,
    })))
}

/// Where an item's PDF probably lives.
///
/// Only rules that are certain: guessing wrong means downloading a login page
/// and calling it a paper, which is worse than asking the user for the address.
pub fn pdf_url_for(item: &yk_core::model::Item) -> Option<String> {
    if let Some(arxiv) = item.field("arxiv").or_else(|| item.field("archiveID")) {
        let id = arxiv.trim().trim_start_matches("arXiv:");
        if !id.is_empty() {
            return Some(format!("https://arxiv.org/pdf/{id}"));
        }
    }

    let url = item.field("url")?.trim();
    if url.is_empty() {
        return None;
    }
    // An arXiv abstract page has a PDF one path segment away.
    if let Some(id) = url.split("arxiv.org/abs/").nth(1) {
        return Some(format!("https://arxiv.org/pdf/{}", id.trim_end_matches('/')));
    }
    if url.ends_with(".pdf") || url.contains("/pdf/") {
        return Some(url.to_string());
    }
    None
}

async fn download_file(url: &str) -> Result<(Vec<u8>, Option<String>)> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(concat!("Yinkote/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::internal(e.to_string()))?
        .get(url)
        .send()
        .await
        .map_err(|e| Error::internal(format!("fetch failed: {e}")))?;

    if !response.status().is_success() {
        return Err(Error::invalid(format!("fetch returned {}", response.status())));
    }

    // Trust the declared length enough to refuse early, and check again after:
    // a server may lie or omit it, and streaming an unbounded body would be the
    // one place a remote host could fill the disk.
    if response.content_length().is_some_and(|n| n > MAX_BYTES) {
        return Err(Error::invalid("file is too large"));
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or(v).trim().to_string());

    let bytes = response.bytes().await.map_err(|e| Error::internal(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::invalid("file is too large"));
    }
    Ok((bytes.to_vec(), content_type))
}

fn attachment_filename(item: &yk_core::model::Item) -> Result<String> {
    if item.item_type != "attachment" {
        return Err(Error::invalid(format!("{} is not an attachment", item.key)));
    }
    item.field("filename")
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .ok_or_else(|| Error::not_found(format!("no file recorded for {}", item.key)))
}

/// Delete stored bytes for items that are being destroyed.
///
/// Called on permanent deletion only. Emptying the trash is the one moment the
/// bytes stop being reachable, and without this the disk would keep every PDF
/// the library ever held.
pub async fn forget_files(app: &App, lib: i64, keys: &[Key]) {
    for key in keys {
        // Children hold the files; the parent may have several.
        if let Ok(children) = app.store().items.children(lib, key).await {
            for child in children {
                let _ = app.storage().remove(&child.key).await;
            }
        }
        let _ = app.storage().remove(key).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::{Fields, Item};

    fn item(fields: &[(&str, &str)]) -> Item {
        let mut map = Fields::new();
        for (k, v) in fields {
            map.insert((*k).into(), json!(v));
        }
        Item {
            key: Key::generate(),
            library_id: 1,
            item_type: "journalArticle".into(),
            parent_key: None,
            fields: map,
            creators: Vec::new(),
            tags: Vec::new(),
            collections: Vec::new(),
            version: 1,
            deleted: false,
            date_added: 0,
            date_modified: 0,
        }
    }

    #[test]
    fn an_arxiv_abstract_page_yields_its_pdf() {
        let got = pdf_url_for(&item(&[("url", "https://arxiv.org/abs/2401.12345")]));
        assert_eq!(got.as_deref(), Some("https://arxiv.org/pdf/2401.12345"));
    }

    #[test]
    fn an_arxiv_id_field_is_enough_on_its_own() {
        let got = pdf_url_for(&item(&[("arxiv", "arXiv:2401.12345")]));
        assert_eq!(got.as_deref(), Some("https://arxiv.org/pdf/2401.12345"));
    }

    #[test]
    fn a_direct_pdf_link_is_used_as_is() {
        let got = pdf_url_for(&item(&[("url", "https://x.test/paper.pdf")]));
        assert_eq!(got.as_deref(), Some("https://x.test/paper.pdf"));
    }

    #[test]
    fn an_ordinary_landing_page_is_not_guessed_at() {
        // Downloading a login page and calling it a paper is worse than asking.
        assert!(pdf_url_for(&item(&[("url", "https://x.test/article/123")])).is_none());
        assert!(pdf_url_for(&item(&[])).is_none());
    }

    #[test]
    fn only_attachments_have_files() {
        let err = attachment_filename(&item(&[("filename", "a.pdf")])).unwrap_err();
        assert!(err.to_string().contains("not an attachment"));
    }
}
