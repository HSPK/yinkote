//! Per-item badges.
//!
//! Separate from the item payload on purpose: badges come from plugins that may
//! be slow, absent or wrong, and a table that waits for them before showing a
//! single row would be worse than one that fills them in a moment later.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use yk_core::plugin::{BadgeDescriptor, BadgeValue};

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .route("/badges", get(descriptors))
        .route("/libraries/:lib/badges", post(resolve))
}

async fn descriptors(State(app): State<App>) -> Json<Vec<BadgeDescriptor>> {
    Json(app.badges.descriptors().await)
}

#[derive(Deserialize)]
struct ResolveBody {
    keys: Vec<String>,
}

async fn resolve(
    State(app): State<App>,
    Path(lib): Path<i64>,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<HashMap<String, Vec<BadgeValue>>>> {
    let mut items = Vec::with_capacity(body.keys.len());
    for raw in &body.keys {
        // A key that no longer exists is simply not annotated; the table may
        // well have moved on since it asked.
        if let Ok(item) = app.store().items.get(lib, &key(raw)?).await {
            items.push(item);
        }
    }
    Ok(Json(app.badges.resolve(&items).await))
}
