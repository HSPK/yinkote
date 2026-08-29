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

    let doi = item.field("DOI").map(str::trim).filter(|d| !d.is_empty()).ok_or_else(|| {
        Error::invalid("only a paper with a DOI has a reference list to fetch")
    })?;

    let found = yk_scrape::resolver::Crossref::default().references(doi).await?;

    // Stored even when nothing resolves to an item today: the whole value of a
    // reference list is that it names what the library is missing.
    let drafts: Vec<CitationDraft> = found.iter().map(to_draft).collect();

    let stored = app.store().relations.set_citations(lib, &key, drafts).await?;
    let cites = app.store().relations.cites(lib, &key).await?;

    Ok(Json(json!({
        "stored": stored,
        "resolved": cites.iter().filter(|c| c.key.is_some()).count(),
        "cites": cites,
    })))
}
