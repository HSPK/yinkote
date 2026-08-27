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
        .route("/libraries/:lib/items/:key/citations", get(list))
        .route("/libraries/:lib/items/:key/citations/fetch", post(fetch))
        .route("/libraries/:lib/citations/missing", get(missing))
        .route("/libraries/:lib/citations/harvest", get(harvest_status).post(start_harvest))
        .route("/libraries/:lib/citations/harvest/stop", post(stop_harvest))
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

/// What a run is doing, for anybody who asks.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Harvest {
    pub running: bool,
    /// Papers this run set out to ask about.
    pub total: u32,
    pub done: u32,
    /// References stored so far.
    pub stored: u64,
    /// Papers whose publisher deposited no reference list. Reported because a
    /// run that stores little is usually not broken — most publishers simply
    /// do not deposit — and a number nobody explains looks like a bug.
    pub empty: u32,
    pub failed: u32,
    pub stopped: bool,
    pub message: Option<String>,
}

async fn harvest_status(State(app): State<App>) -> Json<serde_json::Value> {
    Json(json!(app.harvest.lock().clone()))
}

/// Ask the caller's library to stop the run at the next paper.
///
/// At the next paper, not immediately: a request already in flight is going to
/// arrive whatever we do, and throwing away its answer would mean asking again
/// later for nothing.
async fn stop_harvest(State(app): State<App>) -> Json<serde_json::Value> {
    let mut harvest = app.harvest.lock();
    if harvest.running {
        harvest.stopped = true;
    }
    Json(json!(harvest.clone()))
}

/// Work through every paper whose references have not been fetched.
async fn start_harvest(
    State(app): State<App>,
    Path(lib): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let harvest = app.harvest.lock();
        if harvest.running {
            return Err(Error::invalid("a run is already going").into());
        }
    }

    let pending = app.store().relations.unfetched(lib, HARVEST_CAP).await?;
    if pending.is_empty() {
        return Ok(Json(json!(Harvest { message: Some("nothing to fetch".into()), ..Default::default() })));
    }

    *app.harvest.lock() =
        Harvest { running: true, total: pending.len() as u32, ..Default::default() };

    let worker = app.clone();
    tokio::spawn(async move {
        run_harvest(worker, lib, pending).await;
    });

    Ok(Json(json!(app.harvest.lock().clone())))
}

async fn run_harvest(app: App, lib: i64, pending: Vec<(yk_core::Key, String)>) {
    let crossref = yk_scrape::resolver::Crossref::default();
    let mut consecutive_failures = 0u32;

    for (key, doi) in pending {
        if app.harvest.lock().stopped {
            break;
        }

        match crossref.references(&doi).await {
            Ok(found) => {
                consecutive_failures = 0;
                let drafts: Vec<CitationDraft> = found.iter().map(to_draft).collect();
                let stored = app
                    .store()
                    .relations
                    .set_citations(lib, &key, drafts)
                    .await
                    .unwrap_or(0);

                let mut harvest = app.harvest.lock();
                harvest.stored += stored;
                if stored == 0 {
                    harvest.empty += 1;
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                tracing::info!(error = %e, doi, "reference fetch failed");
                let mut harvest = app.harvest.lock();
                harvest.failed += 1;
                if consecutive_failures >= GIVE_UP_AFTER {
                    harvest.message = Some(format!("stopped after {GIVE_UP_AFTER} failures: {e}"));
                    harvest.stopped = true;
                }
            }
        }

        app.harvest.lock().done += 1;
        tokio::time::sleep(POLITE_PAUSE).await;
    }

    app.harvest.lock().running = false;

    // Announce once at the end rather than per paper: a run of a thousand
    // papers would otherwise be a thousand refreshes of everybody's list.
    let version = app.store().libraries.version(lib).await.unwrap_or_default();
    app.events().publish(DomainEvent::ItemsChanged {
        library_id: lib,
        keys: Vec::new(),
        version,
    });
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
