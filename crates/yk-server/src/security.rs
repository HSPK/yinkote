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

/// The cookie the workbench mirrors its key into.
///
/// Set `SameSite=Strict` by the client, which is what makes this safe to
/// accept: a request originating from any other site carries no cookie at all,
/// so there is no cross-site request to forge with.
pub const COOKIE_NAME: &str = "yk_key";

/// Read one cookie out of a `Cookie` header.
fn cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    raw.split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| percent_decode(v))
}

/// Undo the encoding the client applies, so a key with a `;` or a space in it
/// survives the trip. Anything malformed is left alone: this feeds a
/// constant-time comparison that will simply fail.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

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
    let config = app.config();
    if config.host == "127.0.0.1" || config.host == "::1" {
        let host = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !host.is_empty() && !ALLOWED_HOSTNAMES.contains(&hostname(host)) {
            return deny(StatusCode::FORBIDDEN, "unrecognised Host header");
        }
    }

    if let Some(expected) = &config.api_key {
        let header = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()));
        // A header covers everything the workbench fetches itself. It cannot
        // cover the three things the *browser* fetches on its behalf: an
        // `<img>` for a thumbnail, the stream a PDF viewer opens, and a
        // WebSocket handshake — none of which can carry one. All three
        // answered 401 with a key set, so the page worked and every picture,
        // document and live update did not.
        let presented = match header {
            Some(value) => value.to_string(),
            None => cookie(headers, COOKIE_NAME).unwrap_or_default(),
        };
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

    #[test]
    fn reads_one_cookie_out_of_several() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "theme=dark; yk_key=topsecret; other=1".parse().unwrap(),
        );
        assert_eq!(cookie(&headers, COOKIE_NAME).as_deref(), Some("topsecret"));
        assert_eq!(cookie(&headers, "absent"), None);
    }

    #[test]
    fn a_key_with_awkward_characters_survives_the_cookie() {
        // Cookies cannot hold a `;` or a space, so the client encodes. A key
        // that arrived mangled would fail the comparison and lock the user out
        // of their own library with no clue why.
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "yk_key=a%20b%3Bc%25d".parse().unwrap(),
        );
        assert_eq!(cookie(&headers, COOKIE_NAME).as_deref(), Some("a b;c%d"));
    }

    #[test]
    fn a_malformed_escape_is_left_alone_rather_than_dropped() {
        // It feeds a constant-time comparison that will fail; silently eating
        // bytes would be a way to make two different keys look alike.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
