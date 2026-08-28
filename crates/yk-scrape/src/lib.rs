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
pub mod jsonld;
pub mod mapping;
pub mod meta;
pub mod resolver;

use std::sync::Arc;

use serde::Serialize;
use yk_core::model::ItemDraft;
use yk_core::Error;

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

/// Why an identifier the user gave us produced nothing.
///
/// Without this an unresolved identifier is indistinguishable from a made-up
/// one: the response carries what was detected and an empty result list, and
/// "we could not reach the publisher" reads to the user as "this is not a real
/// paper". The reason is a *code*, not a sentence — the wording belongs in the
/// i18n catalogues, not in the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    /// The source answered, and said no. Nothing here is going to work.
    NotFound,
    /// The source refused *us*: a session, a subscription or a bot wall. The
    /// browser connector is inside that session and is the way through.
    Blocked,
    /// Reachable in principle, not right now. Retrying is reasonable.
    Unavailable,
}

/// An identifier that could not be turned into an item, and why.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unresolved {
    pub kind: String,
    pub identifier: String,
    pub problem: Problem,
    /// The last source's own words. For the log and the tooltip, never the
    /// primary message: it is English, untranslated and often a bare status.
    pub detail: String,
}

/// What a batch of text yielded: the items, and an account of the rest.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub resolutions: Vec<Resolution>,
    pub unresolved: Vec<Unresolved>,
}

/// Higher wins when several sources fail differently. See `try_resolve`.
fn rank(p: Problem) -> u8 {
    match p {
        Problem::NotFound => 0,
        Problem::Unavailable => 1,
        Problem::Blocked => 2,
    }
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
        // Order is preference within an identifier kind: the first to answer
        // wins, and the ones after it are the fallbacks. Crossref knows most
        // journal DOIs; DataCite owns the datasets, software and theses it has
        // never heard of; OpenAlex and Semantic Scholar cover the preprints
        // that never acquired one.
        Self::new(vec![
            Arc::new(resolver::crossref()),
            Arc::new(resolver::datacite()),
            Arc::new(resolver::Arxiv::default()),
            Arc::new(resolver::PubMed::default()),
            Arc::new(resolver::openalex()),
            Arc::new(resolver::semantic_scholar()),
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
        self.try_resolve(identifier).await.ok()
    }

    /// As `resolve`, but keeping the reason when there is nothing to return.
    ///
    /// The reason used to end at a `tracing::warn!`, which is the one place
    /// the person who pasted the link will never look. It is kept as the
    /// *most specific* problem any source reported, so one source being down
    /// cannot mask another having said a definite no.
    pub async fn try_resolve(&self, identifier: &Identifier) -> Result<Resolution, Unresolved> {
        let mut worst: Option<(Problem, String)> = None;
        let mut claimed = false;

        for resolver in self.resolvers.iter().filter(|r| r.handles(identifier)) {
            claimed = true;
            let (problem, detail) = match resolver.resolve(identifier).await {
                Ok(Some(draft)) => {
                    return Ok(Resolution {
                        kind: identifier.kind().to_string(),
                        identifier: identifier.value().to_string(),
                        source: resolver.id(),
                        draft,
                    })
                }
                Ok(None) => {
                    tracing::debug!(%identifier, resolver = resolver.id(), "not found");
                    (Problem::NotFound, format!("{}: no record", resolver.id()))
                }
                Err(e @ Error::Forbidden(_)) => {
                    tracing::info!(%identifier, resolver = resolver.id(), "refused");
                    (Problem::Blocked, e.to_string())
                }
                Err(e) => {
                    tracing::warn!(
                        %identifier, resolver = resolver.id(), error = %e, "resolve failed"
                    );
                    (Problem::Unavailable, e.to_string())
                }
            };
            // Rank, don't overwrite: "blocked" is a fact about this machine and
            // is what the user can act on, so it outranks a bare "not found"
            // from a source that was never going to have it anyway.
            if worst.as_ref().is_none_or(|(p, _)| rank(problem) > rank(*p)) {
                worst = Some((problem, detail));
            }
        }

        let (problem, detail) = worst.unwrap_or_else(|| {
            let detail = if claimed {
                "no source answered".to_string()
            } else {
                format!("no source handles {}", identifier.kind())
            };
            (Problem::NotFound, detail)
        });
        Err(Unresolved {
            kind: identifier.kind().to_string(),
            identifier: identifier.value().to_string(),
            problem,
            detail,
        })
    }

    /// Detect every identifier in `text` and resolve up to `limit` of them,
    /// most specific first.
    ///
    /// A URL that turns out to carry a DOI is upgraded: publisher page metadata
    /// is thinner and less consistent than Crossref's, so we re-resolve and keep
    /// the original URL. That is exactly what a person would do by hand.
    pub async fn resolve_text(&self, text: &str, limit: usize) -> Outcome {
        // Identifiers are resolved together, not one after another. Pasting ten
        // DOIs used to be ten round trips end to end; they have nothing to do
        // with each other, so waiting for each in turn was waiting for nothing.
        //
        // `limit` is applied to the *detected* list first, so the work started
        // is the work that can be returned — resolving everything and throwing
        // most of it away would be worse than the serial version it replaces.
        let found: Vec<Identifier> = detect(text).into_iter().take(limit).collect();
        let resolved = futures_util::future::join_all(found.into_iter().map(|identifier| async move {
            let mut hit = self.try_resolve(&identifier).await?;
            if matches!(identifier, Identifier::Url(_)) {
                if let Some(upgraded) = self.upgrade_via_doi(&hit).await {
                    hit = upgraded;
                }
            }
            Ok(hit)
        }))
        .await;

        // Folded in detection order, which is the order the user wrote them.
        // Concurrency is about when the work happens, not about what comes
        // back — a result that shuffled with network timing would be a
        // different answer every time.
        let mut out = Outcome::default();
        for hit in resolved {
            match hit {
                // Never return the same work twice under two identifiers.
                Ok(hit) => {
                    let seen = fingerprint(&hit.draft);
                    if !out.resolutions.iter().any(|r| fingerprint(&r.draft) == seen) {
                        out.resolutions.push(hit);
                    }
                }
                Err(why) => out.unresolved.push(why),
            }
        }
        out
    }

    /// A failed upgrade is not a failure: the page's own metadata is still a
    /// perfectly good item, so this stays `Option` and the reason is dropped
    /// on purpose.
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
        assert_eq!(hits.resolutions.len(), 1);
        assert_eq!(hits.resolutions[0].source, "doi-source");
        assert_eq!(hits.resolutions[0].kind, "doi");
        assert_eq!(hits.resolutions[0].draft.fields["title"], "Found");
    }

    #[tokio::test]
    async fn a_failing_resolver_falls_through_to_the_next() {
        let engine = ScrapeEngine::new(vec![
            Arc::new(Failing),
            Arc::new(Canned { id: "backup", kinds: &["doi"], answer: Some(draft("Backup")) }),
        ]);
        // A real DOI: the registrant code must be 4-9 digits.
        let hits = engine.resolve_text("10.1000/x", 5).await;
        assert_eq!(hits.resolutions.len(), 1);
        assert_eq!(hits.resolutions[0].source, "backup");
    }

    /// A resolver that records how many calls are in flight at once.
    struct Overlapping {
        peak: Arc<std::sync::atomic::AtomicUsize>,
        live: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Resolver for Overlapping {
        fn id(&self) -> &'static str {
            "slow"
        }
        fn label(&self) -> &'static str {
            "Slow"
        }
        fn supports(&self) -> &'static [&'static str] {
            &["doi"]
        }
        async fn resolve(&self, id: &Identifier) -> yk_core::Result<Option<ItemDraft>> {
            use std::sync::atomic::Ordering;
            let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            self.live.fetch_sub(1, Ordering::SeqCst);
            Ok(Some(draft(id.value())))
        }
    }

    #[tokio::test]
    async fn identifiers_are_resolved_together() {
        // Asserting the answers alone would pass just as well one at a time,
        // so the overlap itself is what is asserted: ten DOIs used to be ten
        // round trips end to end, and they have nothing to do with each other.
        use std::sync::atomic::Ordering;
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let engine = ScrapeEngine::new(vec![Arc::new(Overlapping {
            peak: peak.clone(),
            live: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })]);

        let text = "10.1000/a 10.1000/b 10.1000/c 10.1000/d";
        let started = std::time::Instant::now();
        let hits = engine.resolve_text(text, 10).await;
        let took = started.elapsed();

        assert_eq!(hits.resolutions.len(), 4);
        assert!(peak.load(Ordering::SeqCst) > 1, "the calls did not overlap at all");
        // Four 30ms calls in series would be 120ms. Generous, because a
        // threshold that fails on a busy machine gets switched off.
        assert!(took.as_millis() < 100, "took {took:?}, which looks serial");
    }

    #[tokio::test]
    async fn results_keep_the_order_they_were_written_in() {
        // Concurrency decides when the work happens, not what comes back. An
        // answer that shuffled with network timing would differ every run.
        let engine = ScrapeEngine::new(vec![Arc::new(Overlapping {
            peak: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            live: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })]);
        let hits = engine.resolve_text("10.1000/c 10.1000/a 10.1000/b", 10).await;
        let ids: Vec<&str> = hits.resolutions.iter().map(|h| h.identifier.as_str()).collect();
        assert_eq!(ids, ["10.1000/c", "10.1000/a", "10.1000/b"]);
    }

    #[tokio::test]
    async fn only_what_can_be_returned_is_fetched() {
        // The limit is applied before the work starts. Resolving everything
        // and discarding most of it would be worse than the serial version.
        use std::sync::atomic::Ordering;
        let live = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let engine = ScrapeEngine::new(vec![Arc::new(Overlapping {
            peak: peak.clone(),
            live: live.clone(),
        })]);
        let hits = engine.resolve_text("10.1000/a 10.1000/b 10.1000/c 10.1000/d", 2).await;
        assert_eq!(hits.resolutions.len(), 2);
        assert!(peak.load(Ordering::SeqCst) <= 2, "started more work than it could return");
    }

    #[tokio::test]
    async fn unresolvable_text_yields_nothing_rather_than_an_error() {
        let engine = ScrapeEngine::new(vec![Arc::new(Failing)]);

        // An identifier that no source could serve: no item, but a reason.
        let failed = engine.resolve_text("10.1000/x", 5).await;
        assert!(failed.resolutions.is_empty());
        assert_eq!(failed.unresolved.first().unwrap().problem, Problem::Unavailable);

        // Text with no identifier in it at all: nothing to report either way.
        // There is no failure here, so inventing one would be noise.
        let nothing = engine.resolve_text("no identifiers here", 5).await;
        assert!(nothing.resolutions.is_empty());
        assert!(nothing.unresolved.is_empty(), "reported a problem with text that asked nothing");
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
        assert_eq!(hits.resolutions.len(), 1);
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
        assert_eq!(hits.resolutions.len(), 1);
        assert_eq!(hits.resolutions[0].source, "crossref", "upgraded to the authoritative source");
        assert_eq!(hits.resolutions[0].draft.fields["title"], "Rich metadata");
        assert_eq!(hits.resolutions[0].draft.fields["url"], "https://publisher.example/p", "url preserved");
    }

    #[tokio::test]
    async fn sources_describe_the_registry() {
        let engine = ScrapeEngine::with_defaults();
        let ids: Vec<&str> = engine.sources().iter().map(|s| s.id).collect();
        // Named, not counted. A count asserts the registry has not grown,
        // which is the opposite of what is wanted from a registry whose whole
        // purpose is to grow.
        for expected in ["crossref", "datacite", "arxiv", "pubmed", "openalex", "webpage"] {
            assert!(ids.contains(&expected), "{expected} is missing from the registry");
        }
    }

    /// Refuses, the way a publisher behind a session does.
    struct Refusing;

    #[async_trait]
    impl Resolver for Refusing {
        fn id(&self) -> &'static str {
            "refusing"
        }
        fn label(&self) -> &'static str {
            "refusing"
        }
        fn supports(&self) -> &'static [&'static str] {
            &["url"]
        }
        async fn resolve(&self, _: &Identifier) -> Result<Option<ItemDraft>> {
            Err(yk_core::Error::Forbidden("403 Forbidden".into()))
        }
    }

    #[tokio::test]
    async fn a_refusal_is_reported_as_blocked_not_as_absence() {
        let engine = ScrapeEngine::new(vec![Arc::new(Refusing)]);
        let out = engine.resolve_text("https://paywalled.example/article/1", 3).await;
        assert!(out.resolutions.is_empty());
        let why = out.unresolved.first().expect("no reason was kept");
        assert_eq!(why.problem, Problem::Blocked, "a 403 must not read as 'no such paper'");
        assert_eq!(why.identifier, "https://paywalled.example/article/1");
    }

    #[tokio::test]
    async fn a_definite_no_is_not_dressed_up_as_an_outage() {
        let engine = ScrapeEngine::new(vec![Arc::new(Canned {
            id: "empty",
            kinds: &["doi"],
            answer: None,
        })]);
        let out = engine.resolve_text("10.1234/nope", 3).await;
        assert_eq!(out.unresolved.first().unwrap().problem, Problem::NotFound);
    }

    #[tokio::test]
    async fn a_blocked_source_outranks_a_source_that_merely_had_nothing() {
        // Both are asked; only one tells the user something they can act on.
        // Whichever order they finish in, "blocked" is the answer worth giving.
        let engine = ScrapeEngine::new(vec![
            Arc::new(Canned { id: "empty", kinds: &["url"], answer: None }),
            Arc::new(Refusing),
        ]);
        let out = engine.resolve_text("https://paywalled.example/a", 3).await;
        assert_eq!(out.unresolved.first().unwrap().problem, Problem::Blocked);

        let flipped = ScrapeEngine::new(vec![
            Arc::new(Refusing),
            Arc::new(Canned { id: "empty", kinds: &["url"], answer: None }),
        ]);
        let out = flipped.resolve_text("https://paywalled.example/a", 3).await;
        assert_eq!(out.unresolved.first().unwrap().problem, Problem::Blocked);
    }

    #[tokio::test]
    async fn what_resolved_is_not_also_reported_as_a_problem() {
        let engine = ScrapeEngine::new(vec![Arc::new(Canned {
            id: "ok",
            kinds: &["doi"],
            answer: Some(draft("Fine")),
        })]);
        let out = engine.resolve_text("10.1234/ok", 3).await;
        assert_eq!(out.resolutions.len(), 1);
        assert!(out.unresolved.is_empty(), "a success was also filed as a failure");
    }
}
