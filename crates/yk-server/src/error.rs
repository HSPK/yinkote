//! Maps domain errors onto HTTP responses (RFC 9457 problem details).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use yk_core::{Error, ErrorKind};

/// Newtype so `?` works in handlers while keeping `yk_core` free of HTTP.
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

fn status_for(kind: ErrorKind) -> StatusCode {
    match kind {
        ErrorKind::NotFound => StatusCode::NOT_FOUND,
        ErrorKind::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorKind::Conflict => StatusCode::CONFLICT,
        ErrorKind::VersionConflict => StatusCode::PRECONDITION_FAILED,
        ErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorKind::Forbidden => StatusCode::FORBIDDEN,
        ErrorKind::Unavailable => StatusCode::BAD_GATEWAY,
        ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = status_for(self.0.kind());
        if status.is_server_error() {
            tracing::error!(error = %self.0, "request failed");
        }
        let mut body = json!({
            "code": self.0.code(),
            "status": status.as_u16(),
            "title": self.0.to_string(),
        });
        if let Error::VersionConflict { expected, current } = &self.0 {
            body["expectedVersion"] = json!(expected);
            body["currentVersion"] = json!(current);
        }
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// Give rejections the same envelope as everything else.
///
/// A handler's errors go through [`ApiError`] and come out as
/// `{code, status, title}`. A request that never reached a handler did not:
/// axum answers its own extractor rejections in `text/plain`, so a client met
/// two different error formats depending on how wrong it was. All five kinds
/// were doing it — an unparseable path segment, malformed JSON, a missing
/// content type, a body of the wrong shape, and an unrouted method.
///
/// This matters more here than in most services because the API is the product
/// surface: plugins, the Word add-in and the browser connector all speak it,
/// and a client that parses errors had to special-case the ones that were not
/// JSON.
pub async fn envelope_rejections(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let response = next.run(request).await;
    let status = response.status();

    // Ours already, or not an error at all. Anything JSON came from a handler
    // and is already in the envelope.
    let is_json = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if !status.is_client_error() && !status.is_server_error() || is_json {
        return response;
    }

    let (parts, body) = response.into_parts();
    // Rejection bodies are one short sentence; the cap is only so this cannot
    // become a way to buffer something large.
    let text = match axum::body::to_bytes(body, 8 * 1024).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::new(),
    };

    let body = json!({
        "code": code_for(status),
        "status": status.as_u16(),
        "title": explain(status, text.trim()),
    });
    (parts.status, Json(body)).into_response()
}

/// The `code` a rejection gets, chosen to match what a handler would have
/// produced for the same class of mistake.
fn code_for(status: StatusCode) -> &'static str {
    match status {
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::PAYLOAD_TOO_LARGE => "too_large",
        s if s.is_server_error() => "internal",
        _ => "invalid_input",
    }
}

/// A sentence a client author can act on.
fn explain(status: StatusCode, text: &str) -> String {
    // Axum names the Rust type it could not build — "did not match any variant
    // of untagged enum CreateBody". The type is an implementation detail and
    // tells the reader nothing about their request, so it is replaced rather
    // than passed on.
    if text.contains("did not match any variant") {
        return "invalid request body: a field has the wrong shape for this endpoint".to_string();
    }
    if text.is_empty() {
        return status.canonical_reason().unwrap_or("request failed").to_string();
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_kind_to_a_sensible_status() {
        assert_eq!(status_for(ErrorKind::NotFound), StatusCode::NOT_FOUND);
        assert_eq!(status_for(ErrorKind::Invalid), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status_for(ErrorKind::VersionConflict), StatusCode::PRECONDITION_FAILED);
        assert_eq!(status_for(ErrorKind::Forbidden), StatusCode::FORBIDDEN);
        assert_eq!(status_for(ErrorKind::Internal), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
