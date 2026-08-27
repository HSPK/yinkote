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

use axum::extract::{Path, State};
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
        .route("/tasks", get(list_tasks))
        .route("/tasks/:id", get(get_task))
        .route("/tasks/:id/cancel", post(cancel_task))
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
///
/// Started rather than awaited: nine seconds for the library this was measured
/// on, and a bigger one is minutes. The answer is a task to watch.
async fn export_all(State(app): State<App>) -> Json<serde_json::Value> {
    let task = app.tasks().start("export", "Packing the library");
    let running = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        match export::run(&running).await {
            Ok(made) => running.tasks().finish(&handle, json!(made)),
            Err(e) => running.tasks().fail(&handle, e),
        }
    });
    Json(json!({ "task": task.snapshot() }))
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
) -> Json<serde_json::Value> {
    let path = std::path::PathBuf::from(body.path.trim());
    let task = app.tasks().start("import", "Reading the archive");
    let running = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        match restore::run(&running, &path, &handle).await {
            Ok(done) => running.tasks().finish(&handle, json!(done)),
            Err(e) => running.tasks().fail(&handle, e),
        }
    });
    Json(json!({ "task": task.snapshot() }))
}

/// Every job this server knows about, newest first.
async fn list_tasks(State(app): State<App>) -> Json<serde_json::Value> {
    Json(json!({ "tasks": app.tasks().list() }))
}

async fn get_task(
    State(app): State<App>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    app.tasks()
        .get(&id)
        .map(|t| Json(json!(t)))
        .ok_or_else(|| yk_core::Error::not_found("no such task").into())
}

/// Ask a job to stop. Whether it can is up to the job.
async fn cancel_task(State(app): State<App>, Path(id): Path<String>) -> Json<serde_json::Value> {
    Json(json!({ "cancelled": app.tasks().cancel(&id) }))
}
