//! A paper's references, and who cites it.
//!
//! Fetching is a separate, explicit call rather than something that happens on
//! save. It is a network request to a third party about one paper, and a
//! local-first tool does not make those on its own — the same reason the agent
//! stays off until it is configured.

use std::time::Duration;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::Error;
use yk_store::CitationDraft;

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/items/:key/citations", get(list).put(replace))
        .route("/libraries/:lib/items/:key/citations/fetch", post(fetch))
        .route("/libraries/:lib/citations/missing", get(missing))
        .route("/libraries/:lib/citations/harvest", post(start_harvest))
}

/// How long to wait between requests.
///
/// Crossref allows far more than this. The limit is deliberately not the one
/// they permit: this is a background job on somebody's laptop working through a
/// library nobody is watching, and being a quiet guest on a free service that
/// the whole field depends on is worth more than finishing sooner.
const POLITE_PAUSE: Duration = Duration::from_millis(300);

/// How many failures in a row before giving up.
///
/// A run that keeps hammering a service which is refusing it is the thing that
/// gets clients blocked. Stopping and saying so is better than persevering.
const GIVE_UP_AFTER: u32 = 5;

/// How many papers one run will work through.
const HARVEST_CAP: u32 = 2000;

/// Work through every paper whose references have not been fetched.
///
/// One of the long jobs, and it goes through the same registry as the rest:
/// it used to keep its own `Harvest` struct in the application state, with its
/// own status and stop endpoints and its own polling in the interface. Three
/// ways of saying "something is running" is two too many.
async fn start_harvest(
    State(app): State<App>,
    Path(lib): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    // One at a time: two runs talk to the same service and would only get the
    // client throttled. The registry knows what is running, so it answers this
    // rather than a flag kept beside it.
    if app.tasks().running("harvest") {
        return Err(Error::invalid("a run is already going").into());
    }

    let pending = app.store().relations.unfetched(lib, HARVEST_CAP).await?;
    if pending.is_empty() {
        return Err(Error::invalid("every paper with a DOI has been asked about").into());
    }

    let task = app.tasks().start("harvest", "task.fetchingReferences");
    task.progress("task.fetchingReferences", 0, pending.len() as u64);

    let worker = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        run_harvest(worker, lib, pending, handle).await;
    });

    Ok(Json(json!({ "task": task.snapshot() })))
}

/// What a run has managed so far. Reported as the task's `detail`, because
/// these are numbers only this job has.
#[derive(Default, serde::Serialize)]
struct Progress {
    stored: u64,
    /// Papers whose publisher deposited no reference list. Reported because a
    /// run that stores little is usually not broken — most publishers simply
    /// do not deposit — and a number nobody explains looks like a bug.
    empty: u32,
    failed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

async fn run_harvest(
    app: App,
    lib: i64,
    pending: Vec<(yk_core::Key, String)>,
    task: std::sync::Arc<crate::tasks::Task>,
) {
    let crossref = yk_scrape::resolver::Crossref::default();
    let mut consecutive_failures = 0u32;
    let mut progress = Progress::default();
    let total = pending.len() as u64;
    let mut done = 0u64;
    let mut gave_up = false;

    for (key, doi) in pending {
        // At the next paper, not immediately: a request already in flight is
        // going to arrive whatever we do, and throwing away its answer means
        // asking again later for nothing.
        if task.cancelled() || gave_up {
            break;
        }

        match crossref.references(&doi).await {
            Ok(found) => {
                consecutive_failures = 0;
                let drafts: Vec<CitationDraft> = found.iter().map(to_draft).collect();
                let stored =
                    app.store().relations.set_citations(lib, &key, drafts).await.unwrap_or(0);
                progress.stored += stored;
                if stored == 0 {
                    progress.empty += 1;
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                tracing::info!(error = %e, doi, "reference fetch failed");
                progress.failed += 1;
                if consecutive_failures >= GIVE_UP_AFTER {
                    progress.message =
                        Some(format!("stopped after {GIVE_UP_AFTER} failures: {e}"));
                    gave_up = true;
                }
            }
        }

        done += 1;
        task.progress("task.fetchingReferences", done, total);
        task.detail(json!(progress));
        tokio::time::sleep(POLITE_PAUSE).await;
    }

    // Announce once at the end rather than per paper: a run of a thousand
    // papers would otherwise be a thousand refreshes of everybody's list.
    let version = app.store().libraries.version(lib).await.unwrap_or_default();
    app.events().publish(DomainEvent::ItemsChanged {
        library_id: lib,
        keys: Vec::new(),
        version,
    });

    let summary = json!(progress);
    if task.cancelled() {
        app.tasks().stopped(&task, summary);
    } else {
        app.tasks().finish(&task, summary);
    }
}

fn to_draft(r: &yk_scrape::Reference) -> CitationDraft {
    CitationDraft {
        fingerprint: r.fingerprint().unwrap_or_default(),
        doi: r.doi.clone().unwrap_or_default(),
        label: r.label(),
        year: r.year,
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Limit {
    #[serde(default = "default_missing")]
    limit: u32,
}

fn default_missing() -> u32 {
    50
}

/// What the library keeps citing and does not hold.
async fn missing(
    State(app): State<App>,
    Path(lib): Path<i64>,
    axum::extract::Query(params): axum::extract::Query<Limit>,
) -> ApiResult<Json<serde_json::Value>> {
    let works = app.store().relations.missing(lib, params.limit.clamp(1, 500)).await?;
    Ok(Json(json!({ "works": works })))
}

/// Record a bibliography the caller already has.
///
/// Until this existed, references could only arrive from Crossref — which
/// made the whole citation graph unusable offline, in a field Crossref covers
/// poorly, or for a paper whose reference list somebody already holds in a
/// `.bib` file. Fetching is a convenience; the facts are the point.
///
/// Replaces rather than merges, for the same reason `set_citations` does: a
/// reference list belongs to a printed paper, and merging two versions of one
/// leaves a list that matches neither.
async fn replace(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<Bibliography>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let drafts: Vec<CitationDraft> = body
        .citations
        .iter()
        .map(|c| {
            let doi = c.doi.trim();
            CitationDraft {
                // Normalised here, the one way every other fingerprint in the
                // program is made — a reference recorded with a raw DOI would
                // resolve to nothing and look like the feature was broken.
                fingerprint: match doi.is_empty() {
                    true => String::new(),
                    false => format!("doi:{}", yk_core::text::normalize(doi)),
                },
                doi: doi.to_string(),
                label: c.label.trim().to_string(),
                year: c.year,
            }
        })
        .collect();

    let stored = app.store().relations.set_citations(lib, &key, drafts).await?;

    let version = app.store().libraries.version(lib).await.unwrap_or_default();
    app.events().publish(DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![key],
        version,
    });
    Ok(Json(json!({ "stored": stored })))
}

#[derive(serde::Deserialize)]
struct Bibliography {
    citations: Vec<CitationInput>,
}

#[derive(serde::Deserialize)]
struct CitationInput {
    #[serde(default)]
    doi: String,
    /// What to show when the cited work is not in the library.
    #[serde(default)]
    label: String,
    #[serde(default)]
    year: Option<i64>,
}

async fn list(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let cites = app.store().relations.cites(lib, &key).await?;
    let cited_by = app.store().relations.cited_by(lib, &key).await?;

    Ok(Json(json!({
        "cites": cites,
        "citedBy": cited_by,
        // Told apart because they mean different things to a reader: one is
        // what this paper stands on, the other is what stands on it.
        "resolved": cites.iter().filter(|c| c.key.is_some()).count(),
    })))
}

/// Ask the publisher what this paper cites.
async fn fetch(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = key(&k)?;
    let item = app.store().items.get(lib, &key).await?;

    let (drafts, source) = gather(&app, lib, &item).await?;

    let stored = app.store().relations.set_citations(lib, &key, drafts).await?;
    let cites = app.store().relations.cites(lib, &key).await?;

    Ok(Json(json!({
        "stored": stored,
        "resolved": cites.iter().filter(|c| c.key.is_some()).count(),
        // Which of the three answered, because they are not equally reliable:
        // a publisher's deposit is authoritative, a page read by a machine is
        // a best effort, and a reader deserves to know which they are looking
        // at.
        "source": source,
        "cites": cites,
    })))
}

/// Where a paper's reference list comes from, in order of how much it can be
/// trusted.
///
/// 1. **Crossref** — what the publisher deposited. Authoritative, and absent
///    for roughly half the literature: preprints have no publisher, and plenty
///    of publishers deposit nothing.
/// 2. **Semantic Scholar** — indexes arXiv directly, so it answers for the
///    preprints Crossref cannot. Asked with the DOI when there is one and the
///    arXiv id otherwise.
/// 3. **The PDF itself** — the references are printed on the page whatever
///    anybody deposited. Read conservatively: a wrong reference names a work
///    the author never cited and is indistinguishable from a real one
///    afterwards.
///
/// Each is tried only when the one before it returned nothing, so the cheapest
/// and most reliable answer wins and the file is only read when it has to be.
async fn gather(
    app: &App,
    lib: i64,
    item: &yk_core::model::Item,
) -> Result<(Vec<CitationDraft>, &'static str), Error> {
    let doi = item.field("DOI").map(str::trim).filter(|d| !d.is_empty());
    let arxiv = item
        .field("arXiv")
        .or_else(|| item.field("arxiv"))
        .map(str::trim)
        .filter(|a| !a.is_empty());

    if let Some(doi) = doi {
        let found = yk_scrape::resolver::Crossref::default().references(doi).await?;
        if !found.is_empty() {
            return Ok((found.iter().map(to_draft).collect(), "crossref"));
        }
    }

    if let Some(id) = doi.map(str::to_string).or_else(|| arxiv.map(|a| format!("arXiv:{a}"))) {
        let found = yk_scrape::resolver::SemanticScholar::default().references(&id).await?;
        if !found.is_empty() {
            return Ok((found.iter().map(to_draft).collect(), "semanticscholar"));
        }
    }

    if let Some(drafts) = from_the_paper(app, lib, item).await {
        if !drafts.is_empty() {
            return Ok((drafts, "pdf"));
        }
    }

    if doi.is_none() && arxiv.is_none() {
        return Err(Error::invalid(
            "this item has no DOI, no arXiv id and no readable PDF to take a reference list from",
        ));
    }
    Ok((Vec::new(), "none"))
}

/// Read the reference list off the paper's own pages.
async fn from_the_paper(app: &App, lib: i64, item: &yk_core::model::Item) -> Option<Vec<CitationDraft>> {
    let text = crate::paper::read(app, lib, item).await.text?;
    let printed = yk_pdf::references::references(&text);
    Some(
        printed
            .into_iter()
            .map(|r| CitationDraft {
                // Only what is printed. A fingerprint is claimed when the entry
                // carries a DOI, and never guessed from the words.
                fingerprint: r
                    .doi
                    .as_deref()
                    .map(|d| format!("doi:{}", yk_core::text::normalize(d)))
                    .unwrap_or_default(),
                doi: r.doi.unwrap_or_default(),
                label: r.text,
                year: r.year,
            })
            .collect(),
    )
}
