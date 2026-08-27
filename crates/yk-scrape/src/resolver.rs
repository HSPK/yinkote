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

/// One entry from a paper's reference list.
///
/// A reference is a fact from outside the library — it came from the
/// publisher, not from anything the user did — which is why, unlike a shared
/// tag, it has to be stored rather than derived.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// Normalised, when the publisher deposited one. Most do not: roughly half
    /// of Crossref's reference entries are prose and nothing else.
    pub doi: Option<String>,
    pub title: Option<String>,
    pub year: Option<i64>,
    /// The raw citation string, kept as a label when there is no title.
    pub unstructured: Option<String>,
}

impl Crossref {
    /// The works a paper cites, in the order it cites them.
    ///
    /// Order is kept because a reference list is a numbered thing in the paper
    /// it came from, and renumbering somebody's bibliography is exactly the
    /// kind of quiet damage this project must not do.
    pub async fn references(&self, doi: &str) -> Result<Vec<Reference>> {
        let url = format!("https://api.crossref.org/works/{}", urlencoding(doi));
        let Some(response) = get(&self.http, &url).await? else { return Ok(Vec::new()) };
        let body: serde_json::Value =
            response.json().await.map_err(|e| Error::Unavailable(format!("crossref: {e}")))?;
        Ok(parse_references(&body))
    }
}

/// Pull the reference list out of a Crossref work.
pub fn parse_references(body: &serde_json::Value) -> Vec<Reference> {
    let Some(list) = body.pointer("/message/reference").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    list.iter()
        .map(|entry| {
            let text = |key: &str| {
                entry.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
            };
            Reference {
                doi: text("DOI").map(|d| d.to_lowercase()),
                // Crossref names a journal article's title `article-title`, and
                // a book's `volume-title`; an entry may carry either.
                title: text("article-title")
                    .or_else(|| text("volume-title"))
                    .or_else(|| text("series-title"))
                    .map(str::to_string),
                year: text("year").and_then(|y| y.get(..4)?.parse().ok()),
                unstructured: text("unstructured").map(str::to_string),
            }
        })
        // An entry with neither an identifier nor any words is not a reference,
        // it is a gap in the publisher's deposit.
        .filter(|r| r.doi.is_some() || r.title.is_some() || r.unstructured.is_some())
        .collect()
}

impl Reference {
    /// How the cited work is written down: the same shape an item's
    /// fingerprint takes, so a reference resolves to a library item through the
    /// index that already exists rather than a column invented for it.
    pub fn fingerprint(&self) -> Option<String> {
        let doi = self.doi.as_deref()?;
        Some(format!("doi:{}", yk_core::text::normalize(doi)))
    }

    /// What to show when the cited work is not in the library.
    pub fn label(&self) -> String {
        self.title
            .clone()
            .or_else(|| self.unstructured.clone())
            .or_else(|| self.doi.clone())
            .unwrap_or_default()
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

#[cfg(test)]
mod reference_tests {
    use super::*;

    fn crossref_work() -> serde_json::Value {
        serde_json::json!({
            "message": {
                "reference": [
                    { "key": "1", "DOI": "10.1000/A", "article-title": "First", "year": "2015" },
                    { "key": "2", "volume-title": "A Book", "year": "1999-06" },
                    { "key": "3", "unstructured": "Somebody, in a journal, 2001." },
                    { "key": "4" }
                ]
            }
        })
    }

    #[test]
    fn reads_a_reference_list_in_order() {
        let refs = parse_references(&crossref_work());
        // A reference list is numbered in the paper it came from; renumbering
        // somebody's bibliography is quiet damage.
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].title.as_deref(), Some("First"));
        assert_eq!(refs[1].title.as_deref(), Some("A Book"));
    }

    #[test]
    fn drops_an_entry_that_says_nothing() {
        // An entry with neither identifier nor words is a gap in the
        // publisher's deposit, not a reference.
        assert!(parse_references(&crossref_work()).iter().all(|r| !r.label().is_empty()));
    }

    #[test]
    fn keeps_prose_when_that_is_all_there_is() {
        // Roughly half of Crossref's reference entries are prose and nothing
        // else. Dropping them would understate a paper's bibliography.
        let refs = parse_references(&crossref_work());
        assert_eq!(refs[2].label(), "Somebody, in a journal, 2001.");
        assert!(refs[2].fingerprint().is_none());
    }

    #[test]
    fn addresses_a_cited_work_the_way_an_item_is_addressed() {
        let refs = parse_references(&crossref_work());
        // The same shape `Item::fingerprint` produces, so resolution uses the
        // index that already exists.
        assert_eq!(refs[0].fingerprint().as_deref(), Some("doi:10 1000 a"));
    }

    #[test]
    fn takes_the_year_from_whatever_shape_it_arrived_in() {
        let refs = parse_references(&crossref_work());
        assert_eq!(refs[0].year, Some(2015));
        assert_eq!(refs[1].year, Some(1999));
    }

    #[test]
    fn a_work_with_no_deposited_references_is_not_an_error() {
        assert!(parse_references(&serde_json::json!({ "message": {} })).is_empty());
    }
}
