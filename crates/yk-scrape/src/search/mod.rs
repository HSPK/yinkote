//! Searching the outside world for papers.
//!
//! Distinct from [`crate::resolver`], and the distinction is the whole point.
//! A resolver answers "what is this?" about an identifier somebody already
//! has. Searching answers "what is there?" about a subject nobody has an
//! identifier for — which is what a person means when they ask you to find
//! papers on a topic.
//!
//! The literature-search skill has told the assistant to call
//! `search_external` since it was written, and there was no such thing: every
//! search either failed or fell back to the model's recollection of the field,
//! which is the one source of metadata this program exists to avoid.
//!
//! One [`SearchSource`] per service, each declaring the subjects it covers so
//! a question about public health is not put to a mathematics preprint server.
//! Sources are queried together and merged, because coverage genuinely
//! differs: arXiv has the preprint, Crossref has the version of record,
//! PubMed has the trial nobody else indexes.

use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use yk_core::{Error, Result};

const USER_AGENT: &str =
    concat!("Yinkote/", env!("CARGO_PKG_VERSION"), " (https://github.com/yinkote/yinkote)");

const TIMEOUT: Duration = Duration::from_secs(20);

/// One result, in the terms the next step needs.
///
/// Deliberately not an `ItemDraft`. A search result is a *candidate* — nobody
/// has chosen it, and what a search API returns is usually thinner than what
/// resolving its identifier afterwards gives. Handing back a draft would
/// invite writing it straight into the library, which is the "plausible-
/// looking papers nobody chose" the skill warns against.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    /// What `quick_add` should be given: a DOI if there is one, else an arXiv
    /// id, else the address — that being the order of how well each resolves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Which service found it, so a reader can weigh it.
    pub source: &'static str,
}

impl Found {
    /// How two results from different services are told to be one paper.
    fn same_as(&self, other: &Found) -> bool {
        if let (Some(a), Some(b)) = (&self.identifier, &other.identifier) {
            if a.eq_ignore_ascii_case(b) {
                return true;
            }
        }
        !self.title.is_empty() && normalise_title(&self.title) == normalise_title(&other.title)
    }
}

fn normalise_title(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a source is, for a caller choosing between them.
#[derive(Debug, Clone, Serialize)]
pub struct SearchSourceInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// Broad subject areas, in the words somebody would use for them.
    pub subjects: &'static [&'static str],
}

#[async_trait]
pub trait SearchSource: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// The subjects this service is worth asking. Empty means anything.
    fn subjects(&self) -> &'static [&'static str];
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Found>>;

    fn info(&self) -> SearchSourceInfo {
        SearchSourceInfo { id: self.id(), label: self.label(), subjects: self.subjects() }
    }
}

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .unwrap_or_default()
}

pub(crate) async fn get_json(
    http: &reqwest::Client,
    url: &str,
    who: &str,
) -> Result<serde_json::Value> {
    let response =
        http.get(url).send().await.map_err(|e| Error::Unavailable(format!("{who}: {e}")))?;
    if !response.status().is_success() {
        return Err(Error::Unavailable(format!("{who}: {}", response.status())));
    }
    response.json().await.map_err(|e| Error::Unavailable(format!("{who}: {e}")))
}

pub(crate) async fn get_text(http: &reqwest::Client, url: &str, who: &str) -> Result<String> {
    let response =
        http.get(url).send().await.map_err(|e| Error::Unavailable(format!("{who}: {e}")))?;
    if !response.status().is_success() {
        return Err(Error::Unavailable(format!("{who}: {}", response.status())));
    }
    response.text().await.map_err(|e| Error::Unavailable(format!("{who}: {e}")))
}

pub(crate) fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

mod parse;
mod sources;

pub use parse::{arxiv_feed, crossref_works, openalex_works, pubmed_summaries};
pub use sources::{Arxiv, Crossref, OpenAlex, PubMed};

/// Every source, queried together.
pub struct SearchEngine {
    sources: Vec<std::sync::Arc<dyn SearchSource>>,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl SearchEngine {
    pub fn new(sources: Vec<std::sync::Arc<dyn SearchSource>>) -> Self {
        Self { sources }
    }

    /// The services this build knows.
    ///
    /// Web of Science, Scopus and Embase are deliberately absent: they are
    /// subscription APIs and no key can be shipped with a program anybody can
    /// download. OpenAlex covers much of the same ground openly, and somebody
    /// with a subscription can add theirs as a plugin rather than a fork.
    pub fn with_defaults() -> Self {
        Self::new(vec![
            std::sync::Arc::new(Arxiv::default()),
            std::sync::Arc::new(Crossref::default()),
            std::sync::Arc::new(PubMed::default()),
            std::sync::Arc::new(OpenAlex::default()),
        ])
    }

    pub fn sources(&self) -> Vec<SearchSourceInfo> {
        self.sources.iter().map(|s| s.info()).collect()
    }

    /// Search, optionally naming which sources to ask.
    pub async fn search(&self, query: &str, limit: usize, only: &[String]) -> Outcome {
        let query = query.trim();
        if query.is_empty() {
            return Outcome {
                results: Vec::new(),
                failed: vec![Failure {
                    source: "query".into(),
                    problem: "there is nothing to search for".into(),
                }],
            };
        }

        let chosen: Vec<_> = self
            .sources
            .iter()
            .filter(|s| only.is_empty() || only.iter().any(|o| o.eq_ignore_ascii_case(s.id())))
            .cloned()
            .collect();

        let mut out = Outcome::default();
        if chosen.is_empty() {
            // Said rather than ignored: "search PubMed for X" that quietly
            // searched everything is a different answer to the one asked for,
            // and the reader would never know.
            out.failed.push(Failure {
                source: only.join(", "),
                problem: format!(
                    "no such source; this build has {}",
                    self.sources.iter().map(|s| s.id()).collect::<Vec<_>>().join(", ")
                ),
            });
            return out;
        }

        // Together, not one after another: they are unrelated services and
        // waiting for each in turn is waiting for nothing.
        let limit = limit.clamp(1, 50);
        let answers = futures_util::future::join_all(
            chosen.iter().map(|s| async move { (s.id(), s.search(query, limit).await) }),
        )
        .await;

        // Separated first, so the failures are reported whatever the merge
        // does with the results.
        let mut lists: Vec<Vec<Found>> = Vec::new();
        for (id, answer) in answers {
            match answer {
                Ok(found) => lists.push(found),
                // One service being down must not empty the answer: the
                // sources that answered are still a search, and naming the
                // missing one is what lets somebody judge the gap.
                Err(e) => out.failed.push(Failure { source: id.into(), problem: e.to_string() }),
            }
        }

        // Round by round, not source by source.
        //
        // Concatenating in source order puts every one of the first service's
        // results above every one of the second's, however bad they are. Asked
        // for maternal screen exposure and child development, arXiv answered
        // with astrotourism and multi-armed bandits and those came *above*
        // PubMed's and Crossref's on-topic papers, because arXiv is first in
        // the list. Nobody scrolls past that.
        //
        // Each service ranks its own results and none of them can rank another
        // service's, so the only ordering available without inventing a score
        // is: every source's best, then every source's second. A source with
        // nothing to say contributes nothing to the front.
        let depth = lists.iter().map(Vec::len).max().unwrap_or(0);
        for rank in 0..depth {
            for list in &lists {
                let Some(hit) = list.get(rank) else { continue };
                match out.results.iter_mut().find(|r| r.same_as(hit)) {
                    // Two services, one paper. The copy carrying an identifier
                    // wins, because that is the one that can actually be added.
                    Some(existing) if existing.identifier.is_none() => *existing = hit.clone(),
                    Some(_) => {}
                    None => out.results.push(hit.clone()),
                }
            }
        }
        out
    }
}

/// What a search produced, including what it could not reach.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub results: Vec<Found>,
    /// Sources that could not answer, and why. Never silently dropped.
    pub failed: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub source: String,
    pub problem: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(title: &str, identifier: Option<&str>, source: &'static str) -> Found {
        Found {
            title: title.into(),
            authors: Vec::new(),
            year: None,
            venue: None,
            identifier: identifier.map(str::to_string),
            url: None,
            summary: None,
            source,
        }
    }

    struct Canned(&'static str, Vec<Found>);

    #[async_trait]
    impl SearchSource for Canned {
        fn id(&self) -> &'static str {
            self.0
        }
        fn label(&self) -> &'static str {
            "canned"
        }
        fn subjects(&self) -> &'static [&'static str] {
            &[]
        }
        async fn search(&self, _: &str, _: usize) -> Result<Vec<Found>> {
            Ok(self.1.clone())
        }
    }

    struct Broken(&'static str);

    #[async_trait]
    impl SearchSource for Broken {
        fn id(&self) -> &'static str {
            self.0
        }
        fn label(&self) -> &'static str {
            "broken"
        }
        fn subjects(&self) -> &'static [&'static str] {
            &[]
        }
        async fn search(&self, _: &str, _: usize) -> Result<Vec<Found>> {
            Err(Error::Unavailable("that service is down".into()))
        }
    }

    #[tokio::test]
    async fn one_paper_in_two_services_is_one_result() {
        // Six sources otherwise return forty rows for twelve papers and leave
        // the reader to do the merging.
        let engine = SearchEngine::new(vec![
            std::sync::Arc::new(Canned("a", vec![found("Attention Is All You Need", None, "a")])),
            std::sync::Arc::new(Canned(
                "b",
                vec![found("attention is all you need!", Some("10.1/x"), "b")],
            )),
        ]);
        let out = engine.search("attention", 10, &[]).await;
        assert_eq!(out.results.len(), 1);
        // And the copy that can actually be added is the one kept.
        assert_eq!(out.results[0].identifier.as_deref(), Some("10.1/x"));
    }

    /// Every source's best before any source's second.
    ///
    /// Concatenating in source order put arXiv's answer to a public-health
    /// question -- astrotourism, and a paper on multi-armed bandits -- above
    /// PubMed's and Crossref's on-topic papers, purely because arXiv is first
    /// in the list. Nobody scrolls past that.
    #[tokio::test]
    async fn the_best_of_each_source_comes_first() {
        let engine = SearchEngine::new(vec![
            std::sync::Arc::new(Canned(
                "arxiv",
                vec![found("Astrotourism", Some("1"), "arxiv"), found("Bandits", Some("2"), "arxiv")],
            )),
            std::sync::Arc::new(Canned(
                "pubmed",
                vec![found("Screen time and children", Some("3"), "pubmed")],
            )),
        ]);
        let out = engine.search("screen exposure", 10, &[]).await;
        let order: Vec<&str> = out.results.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(order, vec!["Astrotourism", "Screen time and children", "Bandits"]);
    }

    /// A source with less to say must not leave a hole at the front.
    #[tokio::test]
    async fn a_short_list_does_not_hold_up_the_others() {
        let engine = SearchEngine::new(vec![
            std::sync::Arc::new(Canned("a", vec![found("A1", Some("1"), "a")])),
            std::sync::Arc::new(Canned(
                "b",
                vec![found("B1", Some("2"), "b"), found("B2", Some("3"), "b"), found("B3", Some("4"), "b")],
            )),
        ]);
        let out = engine.search("x", 10, &[]).await;
        let order: Vec<&str> = out.results.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(order, vec!["A1", "B1", "B2", "B3"]);
    }

    #[tokio::test]
    async fn a_service_that_is_down_does_not_empty_the_answer() {
        let engine = SearchEngine::new(vec![
            std::sync::Arc::new(Broken("pubmed")),
            std::sync::Arc::new(Canned("arxiv", vec![found("A paper", Some("2401.1"), "arxiv")])),
        ]);
        let out = engine.search("x", 10, &[]).await;
        assert_eq!(out.results.len(), 1, "the sources that answered are still a search");
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.failed[0].source, "pubmed", "and it says which was missing");
    }

    #[tokio::test]
    async fn naming_a_source_searches_only_that_one() {
        let engine = SearchEngine::new(vec![
            std::sync::Arc::new(Canned("arxiv", vec![found("From arxiv", None, "arxiv")])),
            std::sync::Arc::new(Canned("pubmed", vec![found("From pubmed", None, "pubmed")])),
        ]);
        let out = engine.search("x", 10, &["pubmed".into()]).await;
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].source, "pubmed");
    }

    #[tokio::test]
    async fn an_unknown_source_is_said_rather_than_ignored() {
        let engine = SearchEngine::new(vec![std::sync::Arc::new(Canned(
            "arxiv",
            vec![found("A", None, "arxiv")],
        ))]);
        let out = engine.search("x", 10, &["web-of-science".into()]).await;
        assert!(out.results.is_empty());
        assert_eq!(out.failed.len(), 1);
        assert!(out.failed[0].problem.contains("arxiv"), "and says what there is instead");
    }

    #[tokio::test]
    async fn an_empty_query_searches_nothing() {
        let engine = SearchEngine::with_defaults();
        let out = engine.search("   ", 10, &[]).await;
        assert!(out.results.is_empty());
        assert_eq!(out.failed.len(), 1, "and says why, rather than reporting zero hits");
    }

    /// The subject list is how a question about public health is routed to
    /// PubMed rather than to a mathematics preprint server.
    #[test]
    fn every_source_says_what_it_covers() {
        let sources = SearchEngine::with_defaults().sources();
        assert!(sources.len() >= 4);
        for source in &sources {
            assert!(!source.id.is_empty() && !source.label.is_empty());
        }
        let medical = sources.iter().find(|s| s.id == "pubmed").expect("pubmed");
        assert!(medical.subjects.iter().any(|s| s.contains("medicine")));
    }
}
