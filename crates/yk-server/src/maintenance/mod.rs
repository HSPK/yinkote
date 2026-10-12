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
use axum::Json;
use serde_json::json;

use crate::error::ApiResult;
use crate::state::App;

pub fn api() -> crate::operations::Api {
    crate::api_routes! {
        POST "/maintenance/backup" => run_backup, "backup_library", Write, "Start a database backup task.";
        GET "/maintenance/backups" => list_backups, "list_backups", Read, "List available library backups.";
        GET "/maintenance/integrity" => check_integrity, "check_integrity", Read, "Check missing and orphaned attachment files.";
        POST "/maintenance/export-all" => export_all, "export_library", Write, "Start an archive export task.";
        POST "/maintenance/import-archive" => import_archive, "import_archive", Dangerous, "Import a server-side archive into the library.";
        GET "/tasks" => list_tasks, "list_tasks", Read, "List background tasks.";
        GET "/tasks/:id" => get_task, "get_task", Read, "Read progress or results of a task.";
        POST "/tasks/:id/cancel" => cancel_task, "cancel_task", Write, "Request cancellation of a task.";
    }
}

/// Take a backup now.
///
/// A task like the other long jobs: four seconds on the library this was
/// measured against, and proportional to it. Uniform because a client that has
/// to remember which maintenance actions block and which do not will get it
/// wrong on the one that grew.
async fn run_backup(State(app): State<App>) -> ApiResult<Json<serde_json::Value>> {
    let task = app.tasks().start("backup", "task.backingUp").await?;
    let running = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        match backups::run(&running).await {
            Ok(made) => running.tasks().finish(&handle, json!(made)).await,
            Err(e) => running.tasks().fail(&handle, e).await,
        }
    });
    Ok(Json(json!({ "task": task.snapshot() })))
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
async fn export_all(State(app): State<App>) -> ApiResult<Json<serde_json::Value>> {
    let task = app.tasks().start("export", "task.packing").await?;
    let running = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        match export::run(&running).await {
            Ok(made) => running.tasks().finish(&handle, json!(made)).await,
            Err(e) => running.tasks().fail(&handle, e).await,
        }
    });
    Ok(Json(json!({ "task": task.snapshot() })))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ArchivePath {
    /// Where the `.yinkote` file is, on this machine.
    path: String,
}

/// Read an archive back in, merging it with whatever is already here.
async fn import_archive(
    State(app): State<App>,
    Json(body): Json<ArchivePath>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = std::path::PathBuf::from(body.path.trim());
    let task = app.tasks().start("import", "task.readingArchive").await?;
    let running = app.clone();
    let handle = task.clone();
    tokio::spawn(async move {
        match restore::run(&running, &path, &handle).await {
            Ok((done, false)) => running.tasks().finish(&handle, json!(done)).await,
            // It noticed the flag and stopped. What it managed is real and is
            // reported; what it did not do is what a second run would pick up.
            Ok((done, true)) => running.tasks().stopped(&handle, json!(done)).await,
            Err(e) => running.tasks().fail(&handle, e).await,
        }
    });
    Ok(Json(json!({ "task": task.snapshot() })))
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
