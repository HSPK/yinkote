//! Looking after the library: backups, and checking that the database and the
//! disk still agree with each other.
//!
//! Both exist because of the same worry. This is a local-first program holding
//! the only copy of somebody's research, and there are two ways to lose it: the
//! obvious one, where there is no backup, and the quiet one, where the database
//! confidently lists a PDF that has not been on the disk for a year and nobody
//! finds out until they need it.

pub mod backups;
pub mod integrity;

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
