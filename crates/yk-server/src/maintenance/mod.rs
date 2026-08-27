//! Looking after the library: backups, and checking that the database and the
//! disk still agree with each other.
//!
//! Both exist because of the same worry. This is a local-first program holding
//! the only copy of somebody's research, and there are two ways to lose it: the
//! obvious one, where there is no backup, and the quiet one, where the database
//! confidently lists a PDF that has not been on the disk for a year and nobody
//! finds out until they need it.

pub mod backups;
pub mod export;
pub mod integrity;
pub mod restore;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/maintenance/backup", post(run_backup))
        .route("/maintenance/backups", get(list_backups))
        .route("/maintenance/integrity", get(check_integrity))
        .route("/maintenance/export-all", post(export_all))
        .route("/maintenance/import-archive", post(import_archive))
}

async fn run_backup(State(app): State<App>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!(backups::run(&app).await?)))
}

async fn list_backups(State(app): State<App>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "backups": backups::list(&backups::dir(&app.config().data_dir())) })))
}

async fn check_integrity(State(app): State<App>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!(integrity::check(&app).await?)))
}

/// The whole library as one file, for moving to another machine.
async fn export_all(State(app): State<App>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!(export::run(&app).await?)))
}

#[derive(serde::Deserialize)]
struct ArchivePath {
    /// Where the `.yinkote` file is, on this machine.
    path: String,
}

/// Read an archive back in, merging it with whatever is already here.
async fn import_archive(
    State(app): State<App>,
    Json(body): Json<ArchivePath>,
) -> ApiResult<Json<serde_json::Value>> {
    let done = restore::run(&app, std::path::Path::new(body.path.trim())).await?;
    Ok(Json(json!(done)))
}
