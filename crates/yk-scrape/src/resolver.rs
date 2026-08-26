//! Metadata resolution.
//!
//! One [`Resolver`] per upstream service, selected by identifier kind. Adding a
//! source — a new preprint server, a national library — means implementing the
//! trait and registering it; nothing else changes.
//!
//! Network handling is deliberately thin: every payload is handed straight to
//! the pure mappings in [`crate::mapping`], which is where the tests live.

use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use yk_core::model::ItemDraft;
use yk_core::{Error, Result};

use crate::identify::Identifier;
use crate::{mapping, meta};

/// Identifies this client to upstream APIs, which is expected etiquette and
/// buys better rate limits from Crossref.
const USER_AGENT: &str =
    concat!("Yinkote/", env!("CARGO_PKG_VERSION"), " (https://github.com/yinkote/yinkote)");

const TIMEOUT: Duration = Duration::from_secs(20);
/// Web pages can be enormous; we only need the `<head>`.
const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;

#[async_trait]
pub trait Resolver: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// Identifier kinds this resolver can answer, e.g. `["doi"]`.
    fn supports(&self) -> &'static [&'static str];
    /// `Ok(None)` means "not found here", which is not an error.
    async fn resolve(&self, identifier: &Identifier) -> Result<Option<ItemDraft>>;

    fn handles(&self, identifier: &Identifier) -> bool {
        self.supports().contains(&identifier.kind())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub supports: &'static [&'static str],
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .unwrap_or_default()
}

/// GET a URL, treating 404 as "absent" rather than as a failure.
async fn get(http: &reqwest::Client, url: &str) -> Result<Option<reqwest::Response>> {
    let response = http
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Unavailable(format!("{url}: {e}")))?;
    match response.status() {
        s if s.is_success() => Ok(Some(response)),
        s if s == reqwest::StatusCode::NOT_FOUND || s == reqwest::StatusCode::GONE => Ok(None),
        s => Err(Error::Unavailable(format!("{url}: {s}"))),
    }
}

// ---------------------------------------------------------------------------

pub struct Crossref {
    http: reqwest::Client,
}

impl Default for Crossref {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl Resolver for Crossref {
    fn id(&self) -> &'static str {
        "crossref"
    }
    fn label(&self) -> &'static str {
        "Crossref"
    }
    fn supports(&self) -> &'static [&'static str] {
        &["doi"]
    }

    async fn resolve(&self, identifier: &Identifier) -> Result<Option<ItemDraft>> {
        let url =
            format!("https://api.crossref.org/works/{}", urlencoding(identifier.value()));
        let Some(response) = get(&self.http, &url).await? else { return Ok(None) };
        let body: serde_json::Value =
            response.json().await.map_err(|e| Error::Unavailable(format!("crossref: {e}")))?;
        Ok(body.get("message").and_then(mapping::crossref))
    }
}

// ---------------------------------------------------------------------------

pub struct Arxiv {
    http: reqwest::Client,
}

impl Default for Arxiv {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl Resolver for Arxiv {
    fn id(&self) -> &'static str {
        "arxiv"
    }
    fn label(&self) -> &'static str {
        "arXiv"
    }
    fn supports(&self) -> &'static [&'static str] {
        &["arxiv"]
    }

    async fn resolve(&self, identifier: &Identifier) -> Result<Option<ItemDraft>> {
        let url = format!(
            "https://export.arxiv.org/api/query?id_list={}&max_results=1",
            urlencoding(identifier.value())
        );
        let Some(response) = get(&self.http, &url).await? else { return Ok(None) };
        let body = response.text().await.map_err(|e| Error::Unavailable(format!("arxiv: {e}")))?;
        // The feed wraps entries; an unknown id yields a feed with none.
        let Some(start) = body.find("<entry") else { return Ok(None) };
        let end = body[start..].find("</entry>").map(|e| start + e + 8).unwrap_or(body.len());
        Ok(mapping::arxiv(&body[start..end]))
    }
}

// ---------------------------------------------------------------------------

pub struct OpenLibrary {
    http: reqwest::Client,
}

impl Default for OpenLibrary {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl Resolver for OpenLibrary {
    fn id(&self) -> &'static str {
        "openlibrary"
    }
    fn label(&self) -> &'static str {
        "Open Library"
    }
    fn supports(&self) -> &'static [&'static str] {
        &["isbn"]
    }

    async fn resolve(&self, identifier: &Identifier) -> Result<Option<ItemDraft>> {
        let isbn = identifier.value();
        let url = format!(
            "https://openlibrary.org/api/books?bibkeys=ISBN:{isbn}&format=json&jscmd=data"
        );
        let Some(response) = get(&self.http, &url).await? else { return Ok(None) };
        let body: serde_json::Value =
            response.json().await.map_err(|e| Error::Unavailable(format!("openlibrary: {e}")))?;
        Ok(body.get(format!("ISBN:{isbn}")).and_then(|b| mapping::open_library(b, isbn)))
    }
}

// ---------------------------------------------------------------------------

pub struct PubMed {
    http: reqwest::Client,
}

impl Default for PubMed {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl Resolver for PubMed {
    fn id(&self) -> &'static str {
        "pubmed"
    }
    fn label(&self) -> &'static str {
        "PubMed"
    }
    fn supports(&self) -> &'static [&'static str] {
        &["pmid"]
    }

    async fn resolve(&self, identifier: &Identifier) -> Result<Option<ItemDraft>> {
        let pmid = identifier.value();
        let url = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi\
             ?db=pubmed&retmode=json&id={pmid}"
        );
        let Some(response) = get(&self.http, &url).await? else { return Ok(None) };
        let body: serde_json::Value =
            response.json().await.map_err(|e| Error::Unavailable(format!("pubmed: {e}")))?;
        Ok(body
            .get("result")
            .and_then(|r| r.get(pmid))
            .and_then(|record| mapping::pubmed(record, pmid)))
    }
}

// ---------------------------------------------------------------------------

/// Last resort: read whatever metadata the page itself publishes.
pub struct WebPage {
    http: reqwest::Client,
}

impl Default for WebPage {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl Resolver for WebPage {
    fn id(&self) -> &'static str {
        "webpage"
    }
    fn label(&self) -> &'static str {
        "Web Page"
    }
    fn supports(&self) -> &'static [&'static str] {
        &["url"]
    }

    async fn resolve(&self, identifier: &Identifier) -> Result<Option<ItemDraft>> {
        let url = identifier.value();
        let Some(response) = get(&self.http, url).await? else { return Ok(None) };
        // Follow redirects to wherever we actually landed.
        let final_url = response.url().to_string();
        let body = response.text().await.map_err(|e| Error::Unavailable(format!("{url}: {e}")))?;
        let head = &body[..body.len().min(MAX_HTML_BYTES)];
        Ok(meta::to_draft(&meta::parse(head), &final_url))
    }
}

/// Percent-encode a path segment. Only the characters that actually break a
/// URL — identifiers are otherwise URL-safe by construction.
fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolvers_declare_disjoint_and_complete_coverage() {
        let resolvers: Vec<Box<dyn Resolver>> = vec![
            Box::new(Crossref::default()),
            Box::new(Arxiv::default()),
            Box::new(OpenLibrary::default()),
            Box::new(PubMed::default()),
            Box::new(WebPage::default()),
        ];
        for kind in ["doi", "arxiv", "isbn", "pmid", "url"] {
            let count = resolvers.iter().filter(|r| r.supports().contains(&kind)).count();
            assert_eq!(count, 1, "exactly one resolver should own {kind}");
        }
    }

    #[test]
    fn handles_matches_on_identifier_kind() {
        let crossref = Crossref::default();
        assert!(crossref.handles(&Identifier::Doi("10.1/x".into())));
        assert!(!crossref.handles(&Identifier::Url("https://x".into())));
    }

    #[test]
    fn encodes_only_what_needs_encoding() {
        assert_eq!(urlencoding("10.1038/nature14539"), "10.1038/nature14539");
        assert_eq!(urlencoding("10.1002/(sici)1097"), "10.1002%2F%28sici%291097".replace("%2F", "/"));
        assert!(urlencoding("a b").contains("%20"));
    }
}

#[cfg(test)]
mod label_tests {
    #[test]
    fn resolver_labels_are_language_neutral() {
        // These reach the UI verbatim, which has no way to translate them, so
        // they must read the same to every user: brand names or plain English.
        for r in crate::ScrapeEngine::with_defaults().sources() {
            assert!(
                r.label.is_ascii() && !r.label.is_empty(),
                "resolver {} has a non-neutral label {:?}",
                r.id,
                r.label
            );
        }
    }
}
