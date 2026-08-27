//! Does the database still agree with the disk?
//!
//! Two directions, and they fail differently:
//!
//! - **Missing files.** The library lists a PDF; the disk does not have it.
//!   This is the one that hurts, because nothing announces it — the record
//!   looks perfectly healthy until somebody clicks it, which may be years
//!   after whatever removed the file.
//! - **Orphans.** The disk holds a file no live record points at. Harmless
//!   except that it takes up room, and a library that has had a lot of merging
//!   and trashing done to it accumulates them.
//!
//! Reported, never repaired. Deleting a file because the database cannot
//! account for it is exactly the wrong response when the *database* is what
//! went wrong.

use serde::Serialize;
use yk_core::Result;

use crate::state::App;

/// A record whose file is not where it should be.
#[derive(Debug, Clone, Serialize)]
pub struct Missing {
    pub key: String,
    pub filename: String,
    /// The paper it hangs off, so the report names something a person knows.
    #[serde(rename = "parentTitle")]
    pub parent_title: String,
}

/// A file on disk that no live record accounts for.
#[derive(Debug, Clone, Serialize)]
pub struct Orphan {
    /// Relative to the storage root; an absolute path would leak the home
    /// directory into a report the user may paste somewhere.
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// How many attachment records were examined.
    pub checked: usize,
    pub missing: Vec<Missing>,
    pub orphans: Vec<Orphan>,
    /// What the orphans add up to, which is the number that decides whether
    /// anybody cares about them.
    #[serde(rename = "orphanBytes")]
    pub orphan_bytes: u64,
}

/// How many of each to report. A library with ten thousand broken links has one
/// problem, not ten thousand, and the count says so without the list.
const SAMPLE: usize = 500;

pub async fn check(app: &App) -> Result<Report> {
    let lib = app.store().default_library;
    let attachments = app.store().items.attachments(lib, u32::MAX, 0).await?;

    // Everything the database expects to find, keyed the way the disk stores
    // it: one directory per attachment key.
    let mut expected: std::collections::HashMap<String, String> =
        std::collections::HashMap::with_capacity(attachments.items.len());
    let mut named: Vec<(yk_core::Key, String)> = Vec::new();
    let mut parents: Vec<String> = Vec::new();

    for (attachment, parent) in &attachments.items {
        let filename = attachment.field("filename").unwrap_or_default();
        // A link-only attachment has no bytes here and never did.
        if filename.is_empty() || attachment.field("linkMode") == Some("linked_url") {
            continue;
        }
        expected.insert(attachment.key.to_string(), filename.to_string());
        named.push((attachment.key.clone(), filename.to_string()));
        parents.push(parent.as_ref().map(|p| p.title().to_string()).unwrap_or_default());
    }

    // One trip to the blocking pool for the whole library rather than a `stat`
    // per file awaited in turn — see the file browser, which this is the same
    // shape as.
    let sizes = app.storage().sizes(&named).await;
    let missing: Vec<Missing> = named
        .iter()
        .enumerate()
        .filter(|(i, _)| sizes.get(*i).copied().unwrap_or(0) == 0)
        .take(SAMPLE)
        .map(|(i, (key, filename))| Missing {
            key: key.to_string(),
            filename: filename.clone(),
            parent_title: parents.get(i).cloned().unwrap_or_default(),
        })
        .collect();

    let root = app.config().storage_dir();
    let (orphans, orphan_bytes) =
        tokio::task::spawn_blocking(move || scan_orphans(&root, &expected))
            .await
            .unwrap_or_else(|_| (Vec::new(), 0));

    Ok(Report { checked: named.len(), missing, orphans, orphan_bytes })
}

/// Walk the storage root and report what the database cannot account for.
///
/// Synchronous on purpose, inside one `spawn_blocking`: a directory walk is
/// thousands of small calls, and an async wrapper around each would cost more
/// than the walk.
fn scan_orphans(
    root: &std::path::Path,
    expected: &std::collections::HashMap<String, String>,
) -> (Vec<Orphan>, u64) {
    let mut orphans = Vec::new();
    let mut total = 0u64;

    let Ok(dirs) = std::fs::read_dir(root) else { return (orphans, total) };
    for dir in dirs.filter_map(std::result::Result::ok) {
        if !dir.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let key = dir.file_name().to_string_lossy().to_string();
        let wanted = expected.get(&key);

        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for file in files.filter_map(std::result::Result::ok) {
            let name = file.file_name().to_string_lossy().to_string();
            // The file this key is supposed to hold is not an orphan; anything
            // else in the same directory is — a rename that half happened
            // leaves the old name behind, and that is worth seeing.
            if wanted.is_some_and(|w| crate::storage::safe_filename(w) == name) {
                continue;
            }
            let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
            total += bytes;
            if orphans.len() < SAMPLE {
                orphans.push(Orphan { path: format!("{key}/{name}"), bytes });
            }
        }
    }
    (orphans, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yk-int-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &std::path::Path, key: &str, name: &str, bytes: &[u8]) {
        let dir = root.join(key);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn a_file_the_database_knows_about_is_not_an_orphan() {
        let root = root("known");
        write(&root, "AAAA1111", "paper.pdf", b"x");
        let expected =
            [("AAAA1111".to_string(), "paper.pdf".to_string())].into_iter().collect();

        let (orphans, bytes) = scan_orphans(&root, &expected);
        assert!(orphans.is_empty());
        assert_eq!(bytes, 0);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_left_behind_by_a_rename_is_reported() {
        let root = root("stale");
        // The record now says `Zhang 2020.pdf`; the old name is still there.
        write(&root, "AAAA1111", "paper.pdf", b"12345");
        write(&root, "AAAA1111", "Zhang 2020.pdf", b"12345");
        let expected =
            [("AAAA1111".to_string(), "Zhang 2020.pdf".to_string())].into_iter().collect();

        let (orphans, bytes) = scan_orphans(&root, &expected);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].path, "AAAA1111/paper.pdf");
        assert_eq!(bytes, 5);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn everything_under_a_key_nobody_holds_is_an_orphan() {
        let root = root("gone");
        write(&root, "DEAD0000", "paper.pdf", b"1234567890");
        let (orphans, bytes) = scan_orphans(&root, &Default::default());
        assert_eq!(orphans.len(), 1);
        assert_eq!(bytes, 10);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_report_names_files_relative_to_the_root() {
        // The report is something a user may paste into an issue; it should not
        // carry their home directory with it.
        let root = root("relative");
        write(&root, "DEAD0000", "paper.pdf", b"1");
        let (orphans, _) = scan_orphans(&root, &Default::default());
        assert!(!orphans[0].path.contains(root.to_string_lossy().as_ref()));
        std::fs::remove_dir_all(&root).ok();
    }
}
