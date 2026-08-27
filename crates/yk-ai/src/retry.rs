//! Waiting out a service that is briefly unable to answer.
//!
//! Rate limits are routine and self-healing — the service usually says how
//! long to wait — so failing a request on one throws away work for a condition
//! that resolves itself in a second. This lives here rather than in each
//! caller because only the layer holding the response can read `Retry-After`,
//! and because otherwise every caller would grow the same loop.
//!
//! The failed response is handed back rather than turned into an error, so
//! each caller keeps its own wording for what went wrong.

use std::time::Duration;

use yk_core::{Error, Result};

/// How many times to wait out a busy service before giving up.
///
/// Three, because a limit that has not cleared after three waits is not a
/// blip, and somebody staring at a spinner deserves to be told.
pub const MAX_RETRIES: u32 = 3;

/// The longest a single wait may be, whatever the service asks for.
///
/// A provider that says "retry after 300 seconds" is telling the truth, but
/// nobody is waiting five minutes inside one request.
pub const MAX_WAIT: Duration = Duration::from_secs(20);

/// Send a request, waiting out the failures that are worth waiting out.
///
/// `make` is called again for each attempt because a request body is consumed
/// by sending it.
pub async fn send(make: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        let sent = make().send().await.map_err(|e| Error::internal(e.to_string()))?;
        let status = sent.status();
        if status.is_success() || attempt >= MAX_RETRIES || !is_transient(status.as_u16()) {
            return Ok(sent);
        }

        let wait = retry_after(sent.headers()).unwrap_or_else(|| backoff(attempt));
        tracing::warn!(
            status = status.as_u16(),
            attempt = attempt + 1,
            wait_ms = wait.as_millis() as u64,
            "upstream busy; retrying"
        );
        tokio::time::sleep(wait).await;
        attempt += 1;
    }
}

/// Whether a failure is worth waiting out.
///
/// 429 is the common one; 502/503/504 are a proxy or a model still loading,
/// which is the same kind of "try again shortly". A 400 or a 401 will fail
/// identically no matter how long anyone waits.
pub fn is_transient(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

/// What the service asked for, if it asked.
///
/// `Retry-After` is either seconds or an HTTP date; only the seconds form is
/// read, because the date form needs a clock both ends agree on and is not
/// what these APIs send.
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)
        .or_else(|| headers.get("x-ratelimit-reset-requests"))?
        .to_str()
        .ok()?;
    let seconds: f64 = raw.trim().trim_end_matches('s').parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    // A service that says "0 seconds" means "now", not "spin" — a loop that
    // believes it literally hammers something already struggling.
    Some(Duration::from_millis(((seconds * 1000.0) as u64).max(200)).min(MAX_WAIT))
}

/// Doubling, for a service that did not say.
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64 << attempt.min(5)).min(MAX_WAIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn a_rate_limit_is_worth_waiting_out_and_a_bad_request_is_not() {
        // The distinction is the whole policy: 429 clears on its own, and no
        // amount of waiting will make a malformed request valid.
        assert!(is_transient(429));
        assert!(is_transient(503));
        assert!(!is_transient(400));
        assert!(!is_transient(401));
        assert!(!is_transient(404));
    }

    #[test]
    fn the_service_is_believed_when_it_says_how_long() {
        assert_eq!(retry_after(&headers(&[("retry-after", "2")])).unwrap().as_millis(), 2000);
    }

    #[test]
    fn a_wait_of_zero_still_pauses_rather_than_spinning() {
        let wait = retry_after(&headers(&[("retry-after", "0")])).unwrap();
        assert!(wait.as_millis() >= 200, "{wait:?}");
    }

    #[test]
    fn an_absurd_wait_is_capped() {
        assert_eq!(retry_after(&headers(&[("retry-after", "300")])).unwrap(), MAX_WAIT);
    }

    #[test]
    fn nonsense_is_ignored_rather_than_trusted() {
        assert!(retry_after(&headers(&[("retry-after", "soon")])).is_none());
        assert!(retry_after(&headers(&[("retry-after", "-5")])).is_none());
        assert!(retry_after(&HeaderMap::new()).is_none());
    }

    #[test]
    fn without_a_hint_the_wait_doubles_and_stays_bounded() {
        assert!(backoff(1) > backoff(0));
        assert!(backoff(20) <= MAX_WAIT);
    }
}
