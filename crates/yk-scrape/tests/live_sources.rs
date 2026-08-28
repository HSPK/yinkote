//! Every registered source, against the live service.
//!
//! **Why this is separate from the unit tests.** The mappings are tested
//! offline against recorded payloads, which is right: the shapes are what
//! surprise us, and a recorded shape is a stable thing to assert on. But a
//! recorded payload cannot tell you the *URL* is right. Get the template wrong
//! — a missing path segment, the wrong encoding, a parameter the API renamed —
//! and every offline test still passes while the source answers nothing for
//! everybody. That is a check passing for the wrong reason; this is the check
//! that does not.
//!
//! **Why it is `#[ignore]`.** A test needing the internet has no business in
//! every `cargo test`: it would fail on an aeroplane, in a sealed CI runner,
//! and whenever somebody else's service is briefly unwell. Run it on purpose:
//!
//! ```text
//! cargo test -p yk-scrape --test live_sources -- --ignored --nocapture
//! ```
//!
//! **What counts as a failure.** A source answering nothing for an identifier
//! that certainly exists is a failure — that is our URL being wrong. A source
//! that cannot be reached is *skipped* with a reason, because a broken network
//! is not a broken program, and a check that goes red for somebody else's
//! outage is a check that gets switched off.

use yk_scrape::{Identifier, Resolver};

/// A known-good identifier per source, with something its title must contain.
///
/// Permanent records on purpose. A paper from 2015 with ten thousand citations
/// is not going to be withdrawn; a fixture that can disappear is a test that
/// will one day fail for no reason anybody can act on.
struct Probe {
    source: &'static str,
    identifier: Identifier,
    expect: &'static str,
}

fn probes() -> Vec<(Box<dyn Resolver>, Probe)> {
    use yk_scrape::resolver::*;
    vec![
        (
            Box::new(crossref()),
            Probe {
                source: "crossref",
                identifier: Identifier::Doi("10.1038/nature14539".into()),
                expect: "Deep learning",
            },
        ),
        (
            Box::new(datacite()),
            Probe {
                source: "datacite",
                // A Dryad dataset. Crossref answers 404 for this, which is the
                // entire reason DataCite is in the registry.
                identifier: Identifier::Doi("10.5061/dryad.8515".into()),
                expect: "malaria agent",
            },
        ),
        (
            Box::new(openalex()),
            Probe {
                source: "openalex",
                identifier: Identifier::Doi("10.1038/nature14539".into()),
                expect: "Deep learning",
            },
        ),
        (
            Box::new(semantic_scholar()),
            Probe {
                source: "semanticscholar",
                identifier: Identifier::ArXiv("1706.03762".into()),
                expect: "Attention",
            },
        ),
        (
            Box::new(Arxiv::default()),
            Probe {
                source: "arxiv",
                identifier: Identifier::ArXiv("1706.03762".into()),
                expect: "Attention",
            },
        ),
        (
            Box::new(PubMed::default()),
            Probe {
                source: "pubmed",
                identifier: Identifier::Pmid("30617335".into()),
                expect: "deep learning",
            },
        ),
        (
            Box::new(OpenLibrary::default()),
            Probe {
                source: "openlibrary",
                // Verified against openlibrary.org before being written
                // down. 9780306406157 — the ISBN everybody quotes as an
                // example — is a book about error-correction coding, not the
                // one I remembered it as; the first run of this file caught
                // that, which is a fair advertisement for having it.
                identifier: Identifier::Isbn("9780262510875".into()),
                expect: "Structure and Interpretation",
            },
        ),
        (
            Box::new(WebPage::default()),
            Probe {
                source: "webpage",
                identifier: Identifier::Url("https://arxiv.org/abs/1706.03762".into()),
                expect: "Attention",
            },
        ),
    ]
}

/// Every source in the registry is actually probed above.
///
/// **Not `#[ignore]`, on purpose.** The rest of this file needs the network,
/// so the attribute got applied to the file's subject rather than to each
/// test's real requirement — but *this* question is offline: both lists are
/// built by constructing resolvers, and construction talks to nobody. Left
/// behind `--ignored` it would be answered only by someone who already
/// remembered the live suite exists, which is exactly the person who does not
/// need reminding.
///
/// Without this, adding a ninth resolver leaves the live suite testing eight
/// and reporting success — a check that silently stops covering what it
/// names (§3.200). The registry is *meant* to grow, so the guard has to be
/// the thing that grows with it.
#[test]
fn every_registered_source_has_a_probe() {
    let registered: Vec<&str> =
        yk_scrape::ScrapeEngine::with_defaults().sources().iter().map(|s| s.id).collect();
    let probed: Vec<&str> = probes().iter().map(|(_, p)| p.source).collect();

    let unprobed: Vec<&&str> = registered.iter().filter(|id| !probed.contains(id)).collect();
    assert!(
        unprobed.is_empty(),
        "in the registry but never probed live, so nothing checks their URLs: {unprobed:?}"
    );

    // And the other way: a probe for a source no longer registered is dead
    // weight that still costs a network round trip on every live run.
    let orphaned: Vec<&&str> = probed.iter().filter(|id| !registered.contains(id)).collect();
    assert!(orphaned.is_empty(), "probed but not in the registry: {orphaned:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the live services; run with --ignored"]
async fn every_source_answers_for_something_that_certainly_exists() {
    let mut wrong: Vec<String> = Vec::new();
    let mut unreachable: Vec<String> = Vec::new();
    let total = probes().len();

    for (resolver, probe) in probes() {
        assert_eq!(resolver.id(), probe.source, "a probe is wired to the wrong resolver");
        match resolver.resolve(&probe.identifier).await {
            Ok(Some(draft)) => {
                let title = draft
                    .fields
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_lowercase();
                if title.contains(&probe.expect.to_lowercase()) {
                    println!("  ok        {:<16} {title}", probe.source);
                } else {
                    // It answered, with something else: the mapping read the
                    // wrong field, or the URL addressed the wrong record.
                    wrong.push(format!(
                        "{}: got {title:?}, wanted something containing {:?}",
                        probe.source, probe.expect
                    ));
                }
            }
            // Nothing found for an identifier that certainly exists. This is
            // the failure the whole file is for.
            Ok(None) => wrong.push(format!("{}: no answer for {}", probe.source, probe.identifier)),
            Err(e) => unreachable.push(format!("{}: {e}", probe.source)),
        }
    }

    for reason in &unreachable {
        println!("  skipped   {reason}");
    }
    assert!(wrong.is_empty(), "sources answered wrongly:\n  {}", wrong.join("\n  "));
    assert!(
        unreachable.len() < total,
        "nothing was reachable at all, so this run proved nothing"
    );
}

/// The registry as a whole, through the engine that chooses between sources.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the live services; run with --ignored"]
async fn the_fallback_chain_reaches_past_the_first_source() {
    let engine = yk_scrape::ScrapeEngine::with_defaults();

    // Crossref is asked first for every DOI and does not know this one, so an
    // answer proves the chain continued rather than stopping at the first
    // source that claimed the kind.
    let hits = engine.resolve_text("10.5061/dryad.8515", 1).await;
    assert_eq!(hits.resolutions.len(), 1, "the fallback never ran: {:?}", hits.unresolved);
    assert_ne!(hits.resolutions[0].source, "crossref");
    assert_eq!(hits.resolutions[0].draft.item_type, "dataset", "a dataset is not an article");
}

/// The `webpage` fallback against the publishers people actually paste.
///
/// The probe above proves one URL works. That is not the question this one
/// asks: the fallback's whole job is pages nobody wrote a mapping for, so its
/// coverage is only knowable by trying a spread and looking at what comes
/// back. Run it when touching `meta`/`jsonld`.
///
/// Deliberately **not** asserting a title per site — publishers rewrite pages,
/// and a test that goes red because a journal reworded a heading is a test
/// that gets switched off. It asserts the two things that are our fault:
/// nothing is saved with a bot-wall title, and a page that answers at all
/// yields a title plus either an author or a DOI (otherwise the record is not
/// worth having and `to_draft` should have declined).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the live services; run with --ignored"]
async fn the_web_fallback_handles_real_publisher_pages() {
    use yk_scrape::resolver::WebPage;
    let pages = [
        "https://www.nature.com/articles/nature14539",
        "https://link.springer.com/article/10.1007/s11263-015-0816-y",
        "https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0173664",
        "https://aclanthology.org/N19-1423/",
        "https://www.jmlr.org/papers/v21/20-074.html",
        // Answers 200 with a browser check to anything without a session. The
        // right outcome is no item at all; see `INTERSTITIAL` in `meta`.
        "https://openreview.net/forum?id=YicbFdNTTy",
        "https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)",
    ];

    let web = WebPage::default();
    let mut bad: Vec<String> = Vec::new();
    let mut answered = 0usize;

    for url in pages {
        match web.resolve(&Identifier::Url(url.to_string())).await {
            Ok(Some(draft)) => {
                answered += 1;
                let field = |k: &str| {
                    draft.fields.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string()
                };
                let title = field("title");
                println!("  saved     {:<24} {title}", draft.item_type);
                if is_wall(&title) {
                    bad.push(format!("{url}: saved an interstitial as an item: {title:?}"));
                } else if draft.creators.is_empty() && field("DOI").is_empty() {
                    bad.push(format!("{url}: title only, no author and no DOI: {title:?}"));
                }
            }
            Ok(None) => println!("  declined  {url}"),
            Err(e) => println!("  skipped   {url}: {e}"),
        }
    }

    assert!(bad.is_empty(), "bad records:\n  {}", bad.join("\n  "));
    assert!(answered > 0, "no page answered at all, so this run proved nothing");
}

fn is_wall(title: &str) -> bool {
    let t = title.to_lowercase();
    ["verifying your browser", "just a moment", "attention required", "not found", "access denied"]
        .iter()
        .any(|p| t.contains(p))
}
