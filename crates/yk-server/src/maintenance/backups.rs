//! Backups, and deciding which of them to keep.
//!
//! The retention rule is the interesting part and it is a pure function, so it
//! is tested without a filesystem: keep every backup from the last few days,
//! keep one from each month before that, delete the rest. The shape of that
//! rule matters more than it looks — a rule that only kept the last N would
//! quietly throw away the copy from before whatever went wrong, and "I noticed
//! a month later" is the normal way people notice.

use std::path::{Path, PathBuf};

use serde::Serialize;
use yk_core::{Error, Result};

use crate::state::App;

/// How many of the most recent backups to keep, whatever their dates.
const KEEP_RECENT: usize = 7;

/// Where backups live, under the data directory.
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    pub name: String,
    pub bytes: u64,
    /// Seconds since the epoch, from the file itself rather than its name: a
    /// name says which day it covers, not when it was written.
    #[serde(rename = "modifiedAt")]
    pub modified_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Made {
    pub name: String,
    pub bytes: u64,
    /// What was deleted to make room, so the answer is not silently destructive.
    pub pruned: Vec<String>,
    pub kept: usize,
}

/// Take a backup now, then apply the retention rule.
pub async fn run(app: &App) -> Result<Made> {
    let root = dir(&app.config().data_dir());
    std::fs::create_dir_all(&root)
        .map_err(|e| Error::internal(format!("could not make {}: {e}", root.display())))?;

    let name = name_for(now_utc());
    let path = root.join(&name);
    // Same day, run twice: the second one supersedes the first rather than
    // failing. A backup button that reports an error because you already have
    // today's backup teaches people to ignore errors.
    if path.exists() {
        std::fs::remove_file(&path).ok();
    }
    let bytes = app.store().db().backup_to(path).await?;

    let existing: Vec<String> = list(&root).into_iter().map(|b| b.name).collect();
    let doomed = prune(&existing, KEEP_RECENT);
    for name in &doomed {
        std::fs::remove_file(root.join(name)).ok();
    }

    Ok(Made { name, bytes, kept: existing.len() - doomed.len(), pruned: doomed })
}

/// Every backup on disk, newest first.
pub fn list(root: &Path) -> Vec<Backup> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<Backup> = entries
        .filter_map(|e| e.ok())
        .filter(|e| day_of(&e.file_name().to_string_lossy()).is_some())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(Backup {
                name: e.file_name().to_string_lossy().to_string(),
                bytes: meta.len(),
                modified_at: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            })
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// `yinkote-YYYYMMDD.db`.
fn name_for(day: (i32, u32, u32)) -> String {
    format!("yinkote-{:04}{:02}{:02}.db", day.0, day.1, day.2)
}

/// The `YYYYMMDD` in a backup's name, if it is one.
fn day_of(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("yinkote-")?.strip_suffix(".db")?;
    (rest.len() == 8 && rest.chars().all(|c| c.is_ascii_digit())).then_some(rest)
}

/// Which backups to delete, given every backup's name.
///
/// Keeps the newest `keep_recent`, then one — the newest — from each earlier
/// month. Nothing outside that survives.
///
/// Deliberately returns what to *delete* rather than what to keep: a bug in a
/// function that returns survivors deletes data, and a bug in this one leaves
/// a file lying about. Only one of those is recoverable.
pub fn prune(names: &[String], keep_recent: usize) -> Vec<String> {
    let mut dated: Vec<(&str, &String)> =
        names.iter().filter_map(|n| day_of(n).map(|d| (d, n))).collect();
    dated.sort_by(|a, b| b.0.cmp(a.0));

    let mut seen_months: Vec<&str> = Vec::new();
    let mut doomed = Vec::new();
    for (i, (day, name)) in dated.iter().enumerate() {
        if i < keep_recent {
            // Recent enough to keep outright, but it still stands for its month
            // — otherwise the newest monthly would be kept twice over and an
            // older one would survive in its place.
            let month = &day[..6];
            if !seen_months.contains(&month) {
                seen_months.push(month);
            }
            continue;
        }
        let month = &day[..6];
        if seen_months.contains(&month) {
            doomed.push((*name).clone());
        } else {
            seen_months.push(month);
        }
    }
    doomed
}

/// Today, as `(year, month, day)` in UTC.
///
/// Days from the epoch by the civil-calendar algorithm, so the program does not
/// take a date library for one line.
fn now_utc() -> (i32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs.div_euclid(86_400))
}

pub(crate) fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((y + i64::from(m <= 2)) as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(days: &[&str]) -> Vec<String> {
        days.iter().map(|d| format!("yinkote-{d}.db")).collect()
    }

    #[test]
    fn a_handful_of_backups_are_all_kept() {
        let all = names(&["20260101", "20260102", "20260103"]);
        assert!(prune(&all, 7).is_empty());
    }

    #[test]
    fn older_ones_thin_out_to_one_a_month() {
        let all = names(&[
            // Ten days in March: the seven newest are kept outright.
            "20260310", "20260309", "20260308", "20260307", "20260306", "20260305", "20260304",
            "20260303", "20260302", "20260301", // February, January
            "20260220", "20260210", "20260105", "20260104",
        ]);
        let doomed = prune(&all, 7);

        // March's three oldest go: March is already represented by the recent
        // ones. February and January each keep their newest.
        assert!(doomed.contains(&"yinkote-20260303.db".to_string()));
        assert!(doomed.contains(&"yinkote-20260301.db".to_string()));
        assert!(doomed.contains(&"yinkote-20260210.db".to_string()));
        assert!(doomed.contains(&"yinkote-20260104.db".to_string()));
        assert!(!doomed.contains(&"yinkote-20260220.db".to_string()), "February's newest");
        assert!(!doomed.contains(&"yinkote-20260105.db".to_string()), "January's newest");
    }

    #[test]
    fn a_year_of_daily_backups_leaves_one_a_month() {
        // The point of the rule: the copy from before whatever went wrong is
        // still there a year later, and "I noticed a month late" is normal.
        let mut all = Vec::new();
        for month in 1..=12 {
            for day in 1..=28 {
                all.push(format!("yinkote-2026{month:02}{day:02}.db"));
            }
        }
        let doomed = prune(&all, 7);
        let kept = all.len() - doomed.len();
        assert_eq!(kept, 7 + 11, "seven recent, plus one for each earlier month");
    }

    #[test]
    fn anything_that_is_not_a_backup_is_left_alone() {
        // A user's own file in the backups directory is not ours to delete.
        let all = vec![
            "yinkote-20260101.db".to_string(),
            "notes.txt".to_string(),
            "yinkote-old.db".to_string(),
        ];
        assert!(prune(&all, 0).iter().all(|n| n.starts_with("yinkote-2")));
    }

    #[test]
    fn the_calendar_is_the_gregorian_one() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // A leap day, which is where a hand-rolled calendar goes wrong.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(name_for(civil_from_days(19_782)), "yinkote-20240229.db");
    }
}
