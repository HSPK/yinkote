//! A paper's references, and who cites it.
//!
//! Fetching is a separate, explicit call rather than something that happens on
//! save. It is a network request to a third party about one paper, and a
//! local-first tool does not make those on its own — the same reason the agent
//! stays off until it is configured.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
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
    let drafts: Vec<CitationDraft> = found
        .iter()
        .map(|r| CitationDraft {
            fingerprint: r.fingerprint().unwrap_or_default(),
            doi: r.doi.clone().unwrap_or_default(),
            label: r.label(),
            year: r.year,
        })
        .collect();

    let stored = app.store().relations.set_citations(lib, &key, drafts).await?;
    let cites = app.store().relations.cites(lib, &key).await?;

    Ok(Json(json!({
        "stored": stored,
        "resolved": cites.iter().filter(|c| c.key.is_some()).count(),
        "cites": cites,
    })))
}
