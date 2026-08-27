//! Identifier detection and bibliographic metadata resolution.
//!
//! Paste anything — a DOI, an arXiv link, a publisher URL, an ISBN, or a whole
//! citation copied out of a PDF — and this crate works out what it is and
//! fetches proper metadata for it.
//!
//! The layering is deliberate: [`identify`], [`meta`] and [`mapping`] are pure
//! and exhaustively tested offline; [`resolver`] holds the only code that
//! touches the network.

pub mod identify;
pub mod mapping;
pub mod meta;
pub mod resolver;

use std::sync::Arc;

use serde::Serialize;
use yk_core::model::ItemDraft;

pub use identify::{detect, detect_one, Identifier};
pub use resolver::{parse_references, Reference, Resolver, SourceInfo};

/// One successfully resolved identifier.
#[derive(Debug, Clone, Serialize)]
pub struct Resolution {
    /// `"doi"`, `"arxiv"`, …
    pub kind: String,
    /// The identifier itself, normalised.
    pub identifier: String,
    /// Which resolver answered.
    pub source: &'static str,
    pub draft: ItemDraft,
}

/// Registry of resolvers, tried in order of identifier specificity.
pub struct ScrapeEngine {
    resolvers: Vec<Arc<dyn Resolver>>,
}

impl ScrapeEngine {
    pub fn new(resolvers: Vec<Arc<dyn Resolver>>) -> Self {
        Self { resolvers }
    }

    pub fn with_defaults() -> Self {
        Self::new(vec![
            Arc::new(resolver::Crossref::default()),
            Arc::new(resolver::Arxiv::default()),
            Arc::new(resolver::PubMed::default()),
            Arc::new(resolver::OpenLibrary::default()),
            Arc::new(resolver::WebPage::default()),
        ])
    }

    pub fn sources(&self) -> Vec<SourceInfo> {
        self.resolvers
            .iter()
            .map(|r| SourceInfo { id: r.id(), label: r.label(), supports: r.supports() })
            .collect()
    }

    /// Resolve a single identifier through the first resolver that claims it.
    pub async fn resolve(&self, identifier: &Identifier) -> Option<Resolution> {
        for resolver in self.resolvers.iter().filter(|r| r.handles(identifier)) {
            match resolver.resolve(identifier).await {
                Ok(Some(draft)) => {
                    return Some(Resolution {
                        kind: identifier.kind().to_string(),
                        identifier: identifier.value().to_string(),
                        source: resolver.id(),
                        draft,
                    })
                }
                Ok(None) => tracing::debug!(%identifier, resolver = resolver.id(), "not found"),
                Err(e) => tracing::warn!(
                    %identifier, resolver = resolver.id(), error = %e, "resolve failed"
                ),
            }
        }
        None
    }

    /// Detect every identifier in `text` and resolve up to `limit` of them,
    /// most specific first.
    ///
    /// A URL that turns out to carry a DOI is upgraded: publisher page metadata
    /// is thinner and less consistent than Crossref's, so we re-resolve and keep
    /// the original URL. That is exactly what a person would do by hand.
    pub async fn resolve_text(&self, text: &str, limit: usize) -> Vec<Resolution> {
        let mut out: Vec<Resolution> = Vec::new();
        for identifier in detect(text) {
            if out.len() >= limit {
                break;
            }
            let Some(mut hit) = self.resolve(&identifier).await else { continue };

            if matches!(identifier, Identifier::Url(_)) {
                if let Some(upgraded) = self.upgrade_via_doi(&hit).await {
                    hit = upgraded;
                }
            }
            // Never return the same work twice under two identifiers.
            let seen = fingerprint(&hit.draft);
            if !out.iter().any(|r| fingerprint(&r.draft) == seen) {
                out.push(hit);
            }
        }
        out
    }

    async fn upgrade_via_doi(&self, hit: &Resolution) -> Option<Resolution> {
        let doi = hit.draft.fields.get("DOI")?.as_str()?.to_string();
        let url = hit.draft.fields.get("url").cloned();
        let mut better = self.resolve(&Identifier::Doi(doi)).await?;
        if let Some(url) = url {
            better.draft.fields.insert("url".into(), url);
        }
        Some(better)
    }
}

impl Default for ScrapeEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Cheap duplicate check between drafts from different sources.
fn fingerprint(draft: &ItemDraft) -> String {
    for key in ["DOI", "arXiv", "ISBN", "PMID"] {
        if let Some(v) = draft.fields.get(key).and_then(|v| v.as_str()) {
            return format!("{key}:{}", v.to_lowercase());
        }
    }
    yk_core::text::normalize(draft.fields.get("title").and_then(|v| v.as_str()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use yk_core::Result;

    /// A resolver that answers from a canned value, so engine behaviour can be
    /// tested without the network.
    struct Canned {
        id: &'static str,
        kinds: &'static [&'static str],
        answer: Option<ItemDraft>,
    }

    #[async_trait]
    impl Resolver for Canned {
        fn id(&self) -> &'static str {
            self.id
        }
        fn label(&self) -> &'static str {
            "canned"
        }
        fn supports(&self) -> &'static [&'static str] {
            self.kinds
        }
        async fn resolve(&self, _: &Identifier) -> Result<Option<ItemDraft>> {
            Ok(self.answer.clone())
        }
    }

    struct Failing;

    #[async_trait]
    impl Resolver for Failing {
        fn id(&self) -> &'static str {
            "failing"
        }
        fn label(&self) -> &'static str {
            "failing"
        }
        fn supports(&self) -> &'static [&'static str] {
            &["doi"]
        }
        async fn resolve(&self, _: &Identifier) -> Result<Option<ItemDraft>> {
            Err(yk_core::Error::Unavailable("upstream down".into()))
        }
    }

    fn draft(title: &str) -> ItemDraft {
        ItemDraft::new("journalArticle").with_field("title", title)
    }

    #[tokio::test]
    async fn resolves_through_the_matching_resolver() {
        let engine = ScrapeEngine::new(vec![Arc::new(Canned {
            id: "doi-source",
            kinds: &["doi"],
            answer: Some(draft("Found")),
        })]);
        let hits = engine.resolve_text("10.1038/nature14539", 5).await;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "doi-source");
        assert_eq!(hits[0].kind, "doi");
        assert_eq!(hits[0].draft.fields["title"], "Found");
    }

    #[tokio::test]
    async fn a_failing_resolver_falls_through_to_the_next() {
        let engine = ScrapeEngine::new(vec![
            Arc::new(Failing),
            Arc::new(Canned { id: "backup", kinds: &["doi"], answer: Some(draft("Backup")) }),
        ]);
        // A real DOI: the registrant code must be 4-9 digits.
        let hits = engine.resolve_text("10.1000/x", 5).await;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "backup");
    }

    #[tokio::test]
    async fn unresolvable_text_yields_nothing_rather_than_an_error() {
        let engine = ScrapeEngine::new(vec![Arc::new(Failing)]);
        assert!(engine.resolve_text("10.1000/x", 5).await.is_empty());
        assert!(engine.resolve_text("no identifiers here", 5).await.is_empty());
    }

    #[tokio::test]
    async fn deduplicates_the_same_work_reached_two_ways() {
        let mut with_doi = draft("Same Paper");
        with_doi.fields.insert("DOI".into(), "10.1000/x".into());
        let engine = ScrapeEngine::new(vec![Arc::new(Canned {
            id: "any",
            kinds: &["doi", "url"],
            answer: Some(with_doi),
        })]);
        let hits = engine.resolve_text("https://x.example/p and 10.1000/x", 5).await;
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn a_url_carrying_a_doi_is_upgraded_but_keeps_its_url() {
        let mut page = draft("Thin page metadata");
        page.fields.insert("DOI".into(), "10.1000/x".into());
        page.fields.insert("url".into(), "https://publisher.example/p".into());

        let engine = ScrapeEngine::new(vec![
            Arc::new(Canned {
                id: "crossref",
                kinds: &["doi"],
                answer: Some(draft("Rich metadata")),
            }),
            Arc::new(Canned { id: "webpage", kinds: &["url"], answer: Some(page) }),
        ]);

        let hits = engine.resolve_text("https://publisher.example/p", 5).await;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "crossref", "upgraded to the authoritative source");
        assert_eq!(hits[0].draft.fields["title"], "Rich metadata");
        assert_eq!(hits[0].draft.fields["url"], "https://publisher.example/p", "url preserved");
    }

    #[tokio::test]
    async fn sources_describe_the_registry() {
        let engine = ScrapeEngine::with_defaults();
        let ids: Vec<&str> = engine.sources().iter().map(|s| s.id).collect();
        assert!(ids.contains(&"crossref"));
        assert!(ids.contains(&"webpage"));
        assert_eq!(engine.sources().len(), 5);
    }
}
