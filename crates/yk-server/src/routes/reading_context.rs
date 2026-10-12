use crate::{error::ApiResult, state::App};
use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::Value;

pub fn api() -> crate::operations::Api {
    crate::api_routes! {
        GET "/libraries/:lib/overview" => overview, "library_overview", Read, "Read bounded counts, largest collections and most-used tags.";
        GET "/libraries/:lib/items/:key/context" => context, "read_paper", Read, "Read a paper's metadata, notes, highlights, attachments and reference counts.";
        GET "/libraries/:lib/items/:key/text" => text, "read_paper_text", Read, "Extract available PDF text for reading. May fail when no usable PDF is available.";
    }
}

async fn overview(State(app): State<App>, Path(lib): Path<i64>) -> ApiResult<Json<Value>> {
    Ok(Json(
        crate::agent::reading::overview(app.store(), lib).await?,
    ))
}
async fn context(
    State(app): State<App>,
    Path((lib, key)): Path<(i64, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        crate::agent::reading::read_paper(app.store(), lib, &super::key(&key)?).await?,
    ))
}
async fn text(
    State(app): State<App>,
    Path((lib, key)): Path<(i64, String)>,
) -> ApiResult<Json<Value>> {
    let item = app.store().items.get(lib, &super::key(&key)?).await?;
    let text = crate::paper::require(&app, lib, &item).await?;
    Ok(Json(serde_json::json!({
        "text":text.text,"truncated":text.truncated,"totalChars":text.total_chars,
    })))
}
