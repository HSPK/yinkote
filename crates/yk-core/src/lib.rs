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

/// Parse the `YYYY-MM-DD HH:MM:SS` UTC stamp SQLite databases store.
///
/// Zotero writes its `dateAdded` this way. Returns `None` for anything that is
/// not that shape, because an importer guessing at a timestamp is worse than
/// one admitting it does not know: the caller then falls back to now, which is
/// at least true of the import.
pub fn parse_sql_utc_ms(text: &str) -> Option<i64> {
    let (date, rest) = text.trim().split_once(&[' ', 'T'][..])?;
    let mut d = date.split('-');
    let (y, m, day) = (d.next()?, d.next()?, d.next()?);
    let mut t = rest.trim_end_matches('Z').split(':');
    let (h, min, sec) = (t.next()?, t.next()?, t.next().unwrap_or("0"));

    let date = time::Date::from_calendar_date(
        y.parse().ok()?,
        time::Month::try_from(m.parse::<u8>().ok()?).ok()?,
        day.parse().ok()?,
    )
    .ok()?;
    let time_of_day = time::Time::from_hms(
        h.parse().ok()?,
        min.parse().ok()?,
        sec.split('.').next()?.parse().ok()?,
    )
    .ok()?;
    Some(date.with_time(time_of_day).assume_utc().unix_timestamp() * 1000)
}

/// Format unix milliseconds as an RFC 3339 timestamp (UTC).
pub fn to_rfc3339(ms: i64) -> String {
    let odt = time::OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    odt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod timestamp_tests {
    use super::*;

    #[test]
    fn reads_the_stamp_sqlite_databases_store() {
        // Zotero's `dateAdded`, which is UTC and has no zone marker.
        assert_eq!(parse_sql_utc_ms("2021-07-03 12:34:56"), Some(1_625_315_696_000));
        // Round-trips through our own formatter.
        assert_eq!(to_rfc3339(1_625_315_696_000), "2021-07-03T12:34:56Z");
    }

    #[test]
    fn accepts_the_iso_spelling_and_a_trailing_zone() {
        assert_eq!(
            parse_sql_utc_ms("2021-07-03T12:34:56Z"),
            parse_sql_utc_ms("2021-07-03 12:34:56"),
        );
    }

    #[test]
    fn refuses_rather_than_guesses() {
        // An importer that invents a timestamp is worse than one that admits
        // it does not know and falls back to the time of the import.
        assert_eq!(parse_sql_utc_ms(""), None);
        assert_eq!(parse_sql_utc_ms("2021-07-03"), None, "no time at all");
        assert_eq!(parse_sql_utc_ms("not a date at all"), None);
        assert_eq!(parse_sql_utc_ms("2021-13-40 99:99:99"), None, "in range, but not real");
    }
}
