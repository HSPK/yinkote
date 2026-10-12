//! Search endpoint. Returns hits only; the UI hydrates items from `/items`
//! when it needs full records, and `/items?q=` does both in one round trip.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Serialize;
use yk_core::query::{SearchHit, SearchRequest, SearchStats};

use super::ListParams;
use crate::error::ApiResult;
use crate::state::App;

pub fn api() -> crate::operations::Api {
    crate::api_routes! {
        GET "/libraries/:lib/search" => search, "search_hits", Read, "Return ranked search hits, scores and snippets without hydrating full records.";
        GET "/search/stats" => stats, "search_stats", Read, "Read current index and embedding statistics.";
    }
}

#[derive(Serialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
    /// How many candidates the query produced. `hits` is one page of them.
    total: i64,
    /// Whether `total` is a floor: a retriever filled its candidate pool.
    approximate: bool,
    /// Echoed so the UI can show which strategy actually ran.
    mode: yk_core::query::SearchMode,
    #[serde(rename = "tookMs")]
    took_ms: u64,
}

async fn search(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<SearchResponse>> {
    let started = std::time::Instant::now();
    let query = params.query(lib)?;
    let mode = params.search_mode();
    let hits = app
        .search()
        .search(&SearchRequest {
            text: params.text().to_string(),
            mode,
            filter: query.filter,
            limit: query.limit,
            offset: query.offset,
            highlight: true,
        })
        .await?;
    Ok(Json(SearchResponse {
        total: hits.total,
        approximate: hits.capped,
        hits: hits.hits,
        mode,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

async fn stats(State(app): State<App>) -> ApiResult<Json<SearchStats>> {
    Ok(Json(app.search().stats().await?))
}
