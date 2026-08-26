//! Yinkote domain core.
//!
//! This crate owns the ubiquitous language of the application: entities, value
//! objects, queries and the *ports* (traits) that outer layers implement.
//! It deliberately has no knowledge of SQL, HTTP, or any concrete engine.

pub mod error;
pub mod event;
pub mod id;
pub mod model;
pub mod plugin;
pub mod ports;
pub mod query;
pub mod schema;
pub mod text;

pub use error::{Error, ErrorKind, Result};
pub use id::Key;

/// Current wall-clock time as unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Format unix milliseconds as an RFC 3339 timestamp (UTC).
pub fn to_rfc3339(ms: i64) -> String {
    let odt = time::OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    odt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
