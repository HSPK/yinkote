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
