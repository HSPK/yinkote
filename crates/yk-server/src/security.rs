//! Request guards for a service that listens on localhost.
//!
//! Two concerns, both cheap:
//!
//! 1. **DNS rebinding.** A malicious page can point a hostname at 127.0.0.1 and
//!    talk to us from the browser. Pinning the `Host` header defeats that.
//! 2. **Optional bearer token**, for when the user deliberately exposes the
//!    server beyond the loopback interface.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::state::App;

const ALLOWED_HOSTNAMES: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

fn deny(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "code": "forbidden", "title": message }))).into_response()
}

/// Hostname part of a `Host` header, dropping the port.
fn hostname(host: &str) -> &str {
    match host.rfind(':') {
        // Keep bracketed IPv6 literals intact.
        Some(i) if !host.ends_with(']') => &host[..i],
        _ => host,
    }
}

pub async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let headers = request.headers();

    // When bound to loopback, only loopback names may address us.
    if app.config.host == "127.0.0.1" || app.config.host == "::1" {
        let host = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !host.is_empty() && !ALLOWED_HOSTNAMES.contains(&hostname(host)) {
            return deny(StatusCode::FORBIDDEN, "unrecognised Host header");
        }
    }

    if let Some(expected) = &app.config.api_key {
        let presented = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()))
            .unwrap_or("");
        if !constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
            return deny(StatusCode::UNAUTHORIZED, "missing or invalid API key");
        }
    }

    next.run(request).await
}

/// Comparison whose duration does not depend on where the first difference is.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ports_but_keeps_ipv6_literals() {
        assert_eq!(hostname("localhost:23130"), "localhost");
        assert_eq!(hostname("127.0.0.1"), "127.0.0.1");
        assert_eq!(hostname("[::1]:23130"), "[::1]");
        assert_eq!(hostname("[::1]"), "[::1]");
    }

    #[test]
    fn rejects_foreign_hostnames() {
        assert!(ALLOWED_HOSTNAMES.contains(&hostname("localhost:1")));
        assert!(!ALLOWED_HOSTNAMES.contains(&hostname("evil.example.com")));
    }

    #[test]
    fn constant_time_eq_behaves_like_eq() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(constant_time_eq(b"", b""));
    }
}
