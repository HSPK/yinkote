//! Taking the whole library somewhere else.
//!
//! A local-first program has to be able to hand the data back. Not as a
//! proprietary blob — as a zip anybody can open, holding a SQLite database
//! anybody can read and the attachments as ordinary files under the names they
//! already have.
//!
//! Distinct from a backup, which is one file for putting *back*. This is one
//! file for moving to another machine, and the difference that matters is the
//! attachments: a backup without them restores a library that has forgotten
//! every PDF in it.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use yk_core::{Error, Result};

use crate::state::App;

#[derive(Debug, Clone, Serialize)]
pub struct Archive {
    pub name: String,
    pub bytes: u64,
    /// How many attachment files travelled with the database.
    pub files: u64,
    /// Files the database expected and the disk did not have. Reported rather
    /// than fatal — a library with a broken link should still be movable, and
    /// `GET /maintenance/integrity` is where that is looked into.
    pub missing: u64,
}

/// What goes in the manifest, so the other end knows what it is opening.
#[derive(Serialize)]
struct Manifest {
    format: &'static str,
    version: u32,
    /// Seconds since the epoch.
    #[serde(rename = "exportedAt")]
    exported_at: i64,
    #[serde(rename = "appVersion")]
    app_version: &'static str,
    items: i64,
    files: u64,
}

/// Where exports are written.
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("exports")
}

/// Write the whole library as one file.
pub async fn run(app: &App) -> Result<Archive> {
    let data_dir = app.config().data_dir();
    let root = dir(&data_dir);
    std::fs::create_dir_all(&root)
        .map_err(|e| Error::internal(format!("could not make {}: {e}", root.display())))?;

    // The database is copied through the same consistent snapshot a backup
    // uses, into a temporary file, so that a library being written to while it
    // is exported still arrives whole.
    let staged = root.join(format!(".staging-{}.db", std::process::id()));
    std::fs::remove_file(&staged).ok();
    app.store().db().backup_to(staged.clone()).await?;

    let storage_root = app.config().storage_dir();
    let name = format!("yinkote-{}.yinkote", stamp());
    let target = root.join(&name);
    let staged_for_zip = staged.clone();
    let out = target.clone();

    // One handoff to the blocking pool for the whole archive: zipping is
    // thousands of file reads and a compressor, none of which yields.
    let written = tokio::task::spawn_blocking(move || {
        write_archive(&out, &staged_for_zip, &storage_root)
    })
    .await
    .map_err(|e| Error::internal(format!("export did not finish: {e}")))??;

    std::fs::remove_file(&staged).ok();
    Ok(Archive {
        name,
        bytes: std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0),
        files: written.0,
        missing: written.1,
    })
}

/// Returns `(files written, files missing)`.
fn write_archive(target: &Path, database: &Path, storage_root: &Path) -> Result<(u64, u64)> {
    // Counted from the snapshot, not from the live library. Asking the running
    // database how many items it has answers a *different question* — it has
    // moved on since the snapshot was taken — and a manifest that disagrees
    // with the database beside it is worse than no manifest.
    let items = yk_store::item_count_of(database);
    let file = std::fs::File::create(target)
        .map_err(|e| Error::internal(format!("could not write {}: {e}", target.display())))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // PDFs are already compressed; spending CPU on them buys almost nothing.
    let stored: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let fail = |e: zip::result::ZipError| Error::internal(format!("archive failed: {e}"));

    zip.start_file("db.sqlite", options).map_err(fail)?;
    let mut source = std::fs::File::open(database)
        .map_err(|e| Error::internal(format!("could not read the snapshot: {e}")))?;
    std::io::copy(&mut source, &mut zip)
        .map_err(|e| Error::internal(format!("could not copy the database: {e}")))?;

    let mut files = 0u64;
    let mut missing = 0u64;
    if let Ok(dirs) = std::fs::read_dir(storage_root) {
        for entry in dirs.filter_map(std::result::Result::ok) {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let key = entry.file_name().to_string_lossy().to_string();
            let Ok(inner) = std::fs::read_dir(entry.path()) else {
                missing += 1;
                continue;
            };
            for f in inner.filter_map(std::result::Result::ok) {
                let name = f.file_name().to_string_lossy().to_string();
                // The path the other end will rebuild: one directory per
                // attachment key, exactly as it is on disk here.
                zip.start_file(format!("library/{key}/{name}"), stored).map_err(fail)?;
                match std::fs::File::open(f.path()) {
                    Ok(mut r) => {
                        std::io::copy(&mut r, &mut zip)
                            .map_err(|e| Error::internal(format!("could not copy a file: {e}")))?;
                        files += 1;
                    }
                    Err(_) => missing += 1,
                }
            }
        }
    }

    let manifest = Manifest {
        format: "yinkote-archive",
        version: 1,
        exported_at: now_secs(),
        app_version: env!("CARGO_PKG_VERSION"),
        items,
        files,
    };
    zip.start_file("manifest.json", options).map_err(fail)?;
    let text = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".into());
    zip.write_all(text.as_bytes())
        .map_err(|e| Error::internal(format!("could not write the manifest: {e}")))?;

    zip.finish().map_err(fail)?;
    Ok((files, missing))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `YYYYMMDD-HHMMSS`, so two exports on one day do not collide.
fn stamp() -> String {
    let secs = now_secs();
    let (y, m, d) = crate::maintenance::backups::civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yk-exp-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_archive_holds_the_database_and_every_file() {
        let dir = scratch("whole");
        let db = dir.join("db.sqlite");
        std::fs::write(&db, b"SQLite format 3\0pretend").unwrap();

        let storage = dir.join("storage");
        std::fs::create_dir_all(storage.join("AAAA1111")).unwrap();
        std::fs::write(storage.join("AAAA1111/paper.pdf"), b"%PDF-1.4").unwrap();
        std::fs::create_dir_all(storage.join("BBBB2222")).unwrap();
        std::fs::write(storage.join("BBBB2222/data.csv"), b"a,b\n1,2\n").unwrap();

        let out = dir.join("out.yinkote");
        let (files, missing) = write_archive(&out, &db, &storage).unwrap();
        assert_eq!(files, 2);
        assert_eq!(missing, 0);

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let names: Vec<String> = zip.file_names().map(str::to_string).collect();
        assert!(names.contains(&"db.sqlite".to_string()));
        assert!(names.contains(&"manifest.json".to_string()));
        // The attachment's path is rebuildable: one directory per key, under
        // `library/`, which is what the other end reads back.
        assert!(names.contains(&"library/AAAA1111/paper.pdf".to_string()));
        assert!(names.contains(&"library/BBBB2222/data.csv".to_string()));

        let mut manifest = String::new();
        use std::io::Read;
        zip.by_name("manifest.json").unwrap().read_to_string(&mut manifest).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(parsed["format"], "yinkote-archive");
        // Counted from the snapshot itself: this one is not a real database,
        // so it reports nothing rather than inventing a number.
        assert_eq!(parsed["items"], 0);
        assert_eq!(parsed["files"], 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_library_with_no_files_still_exports() {
        // A brand new library, or one that only holds links.
        let dir = scratch("empty");
        let db = dir.join("db.sqlite");
        std::fs::write(&db, b"x").unwrap();
        let out = dir.join("out.yinkote");

        let (files, missing) = write_archive(&out, &db, &dir.join("nothing-here")).unwrap();
        assert_eq!(files, 0);
        assert_eq!(missing, 0);
        assert!(zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_exports_in_one_day_do_not_collide() {
        // A name with only a date in it overwrites the morning's export with
        // the afternoon's, which is the one thing an archive must not do.
        let name = stamp();
        assert_eq!(name.len(), 15, "YYYYMMDD-HHMMSS");
        assert!(name.contains('-'));
    }
}
