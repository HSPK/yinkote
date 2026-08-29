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

    let attachment = attach_url(&app, lib, &parent, &url, item.title()).await?;

    announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![parent.clone(), attachment.key.clone()],
        version,
    })
    .await?;
    Ok(Json(json!({ "attachment": attachment, "url": url })))
}

/// Download a file and hang it off an item.
///
/// Shared with the browser connector, which saves a page's PDF the same way a
/// user does from the workbench — the only difference being who asked.
pub(crate) async fn attach_url(
    app: &App,
    lib: i64,
    parent: &Key,
    url: &str,
    title: &str,
) -> Result<yk_core::model::Item> {
    let (mut bytes, mut content_type) = download_file(url).await?;
    let mut source = url.to_string();

    // A pasted address is usually the page *about* the paper, not the paper.
    // Publishers and repositories all advertise the file in the page's head,
    // so follow that once rather than storing the HTML and calling it a PDF —
    // silently attaching a landing page is how a library fills up with files
    // that will not open.
    if is_html(content_type.as_deref()) {
        let html = String::from_utf8_lossy(&bytes);
        match pdf_link_in_html(&html, url) {
            Some(found) => {
                let (b, ct) = download_file(&found).await?;
                if is_html(ct.as_deref()) {
                    return Err(Error::invalid(format!(
                        "{found} is a web page, not a file"
                    )));
                }
                bytes = b;
                content_type = ct;
                source = found;
            }
            None => {
                return Err(Error::invalid(
                    "that address is a web page with no file linked from it",
                ));
            }
        }
    }

    let url = source.as_str();
    let filename = filename_from_url(url, content_type.as_deref());

    // One attachment per source URL: fetching twice must not litter the item
    // with duplicates, which is easy to do by double-clicking.
    let existing = app
        .store()
        .items
        .children(lib, parent)
        .await?
        .into_iter()
        .find(|c| c.item_type == "attachment" && c.field("url") == Some(url));

    let attachment = match existing {
        Some(found) => {
            app.storage().put(&found.key, &filename, &bytes).await?;
            found
        }
        None => {
            let mut draft = ItemDraft::new("attachment")
                .with_field("title", title)
                .with_field("filename", filename.as_str())
                .with_field("contentType", content_type.as_deref().unwrap_or("application/pdf"))
                .with_field("linkMode", "imported_url")
                .with_field("url", url);
            draft.parent_key = Some(parent.clone());
            let created = app.store().items.create(lib, draft).await?;
            app.storage().put(&created.key, &filename, &bytes).await?;
            created
        }
    };
    Ok(attachment)
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

fn is_html(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|c| c.starts_with("text/html") || c.starts_with("application/xhtml"))
}

/// The file a landing page is advertising, if it names one.
///
/// `citation_pdf_url` is the tag Google Scholar reads, which is why nearly
/// every publisher and repository emits it; the `<link>` form is the fallback
/// a few of them use instead. Only these two — a heuristic that scrapes any
/// `.pdf`-looking href finds the "download this issue" link just as happily.
pub fn pdf_link_in_html(html: &str, base: &str) -> Option<String> {
    let found = meta_content(html, "citation_pdf_url")
        .or_else(|| meta_content(html, "citation_pdf_URL"))
        .or_else(|| pdf_link_rel(html))?;
    Some(absolute(&found, base))
}

/// The `content` of a `<meta name="…">`, whichever order the attributes are in.
fn meta_content(html: &str, name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let needle = name.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(at) = lower[from..].find("<meta") {
        let start = from + at;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        if attr(tag, "name").is_some_and(|n| n.eq_ignore_ascii_case(&needle)) {
            if let Some(c) = attr(tag, "content").filter(|c| !c.is_empty()) {
                return Some(c);
            }
        }
        from = end.max(start + 5);
    }
    None
}

fn pdf_link_rel(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(at) = lower[from..].find("<link") {
        let start = from + at;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        let is_pdf = attr(tag, "type").is_some_and(|t| t.eq_ignore_ascii_case("application/pdf"));
        if is_pdf {
            if let Some(href) = attr(tag, "href").filter(|h| !h.is_empty()) {
                return Some(href);
            }
        }
        from = end.max(start + 5);
    }
    None
}

/// One attribute out of a tag, quoted either way.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0usize;
    loop {
        let at = from + lower[from..].find(&format!("{name}="))?;
        // Must be a whole attribute name, not the tail of another one.
        let boundary = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[at + name.len() + 1..];
        if boundary {
            let quote = rest.chars().next()?;
            return if quote == '"' || quote == '\'' {
                rest[1..].find(quote).map(|e| rest[1..1 + e].trim().to_string())
            } else {
                Some(
                    rest.split([' ', '\t', '\n', '\r', '>'])
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                )
            };
        }
        from = at + name.len() + 1;
    }
}

/// Resolve a possibly-relative address against the page it came from.
fn absolute(href: &str, base: &str) -> String {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let Some(scheme_end) = base.find("://") else { return href.to_string() };
    let origin_end = base[scheme_end + 3..].find('/').map(|i| scheme_end + 3 + i);
    if href.starts_with('/') {
        let origin = origin_end.map_or(base, |i| &base[..i]);
        return format!("{origin}{href}");
    }
    let dir = base.rfind('/').filter(|i| *i > scheme_end + 2).map_or(base, |i| &base[..i]);
    format!("{dir}/{href}")
}

/// Why a download failed, as a word the interface can translate.
///
/// The list used to show reqwest's own sentence — "fetch failed: error sending
/// request for url (…)" — which is developer English, is never in the
/// catalogues, and does not say which of the several quite different things
/// went wrong. "Not found" means fix the link; "unreachable" means check the
/// network; "too large" means nothing will ever change. The raw text is kept
/// after the word, for the person who wants it.
fn why_failed(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() || e.is_request() {
        "unreachable"
    } else {
        "failed"
    }
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
        .map_err(|e| Error::internal(format!("{}: {e}", why_failed(&e))))?;

    if !response.status().is_success() {
        // Named, because these mean different things to the person looking at
        // the list: a 404 is a dead link to fix, a 403 is a paywall that no
        // amount of retrying will get past.
        let status = response.status();
        let word = match status.as_u16() {
            404 | 410 => "notFound",
            401 | 403 | 451 => "refused",
            429 => "throttled",
            500..=599 => "serverError",
            _ => "failed",
        };
        return Err(Error::invalid(format!("{word}: {status}")));
    }

    // Trust the declared length enough to refuse early, and check again after:
    // a server may lie or omit it, and streaming an unbounded body would be the
    // one place a remote host could fill the disk.
    if response.content_length().is_some_and(|n| n > MAX_BYTES) {
        return Err(Error::invalid("tooLarge: file is too large"));
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or(v).trim().to_string());

    let bytes = response.bytes().await.map_err(|e| Error::internal(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::invalid("tooLarge: file is too large"));
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
    // One query for every parent's children, and one pass over the disk. Asking
    // per item is thousands of round trips when somebody empties the trash,
    // and it happens inside the request that deletes them.
    let children = app.store().items.children_of(lib, keys).await.unwrap_or_default();

    // Children hold the files; the parent may have several, and may have one of
    // its own.
    let mut all: Vec<Key> = children.into_iter().map(|c| c.key).collect();
    all.extend_from_slice(keys);
    app.storage().remove_many(&all).await;
    // Derived from those bytes, so it goes with them: a cached page that
    // outlives its PDF is a picture of something the library no longer has.
    super::thumbnails::forget(app, &all).await;
}

#[cfg(test)]
mod landing_page_tests {
    use super::{is_html, pdf_link_in_html};

    #[test]
    fn finds_the_tag_google_scholar_reads() {
        let html = r#"<html><head>
            <meta name="citation_title" content="Attention Is All You Need">
            <meta name="citation_pdf_url" content="https://arxiv.org/pdf/1706.03762">
        </head></html>"#;
        assert_eq!(
            pdf_link_in_html(html, "https://arxiv.org/abs/1706.03762").as_deref(),
            Some("https://arxiv.org/pdf/1706.03762")
        );
    }

    #[test]
    fn attribute_order_does_not_matter() {
        let html = r#"<meta content="/files/paper.pdf" name="citation_pdf_url" />"#;
        assert_eq!(
            pdf_link_in_html(html, "https://example.org/journal/article/1").as_deref(),
            Some("https://example.org/files/paper.pdf")
        );
    }

    #[test]
    fn single_quotes_and_relative_paths_resolve() {
        let html = "<meta name='citation_pdf_url' content='paper.pdf'>";
        assert_eq!(
            pdf_link_in_html(html, "https://example.org/journal/article").as_deref(),
            Some("https://example.org/journal/paper.pdf")
        );
    }

    #[test]
    fn falls_back_to_the_link_element() {
        let html = r#"<link rel="alternate" type="application/pdf" href="https://x.org/a.pdf">"#;
        assert_eq!(
            pdf_link_in_html(html, "https://x.org/page").as_deref(),
            Some("https://x.org/a.pdf")
        );
    }

    #[test]
    fn a_page_advertising_nothing_is_not_guessed_at() {
        // Better to say so than to attach the login page as the paper.
        let html = r#"<html><body><a href="/download/issue.pdf">whole issue</a></body></html>"#;
        assert_eq!(pdf_link_in_html(html, "https://x.org/page"), None);
    }

    #[test]
    fn a_similarly_named_attribute_is_not_mistaken_for_the_tag() {
        let html = r#"<meta property="og:citation_pdf_url" content="https://wrong.example/x.pdf">"#;
        assert_eq!(pdf_link_in_html(html, "https://x.org/page"), None);
    }

    #[test]
    fn charset_parameters_do_not_hide_html() {
        assert!(is_html(Some("text/html")));
        assert!(!is_html(Some("application/pdf")));
        assert!(!is_html(None));
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
            attachments: Vec::new(),
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
