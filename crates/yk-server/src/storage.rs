//! Attachment files.
//!
//! Files live on disk, not in SQLite: a library of PDFs is measured in
//! gigabytes, and putting them in the database would make every backup, sync
//! and vacuum proportional to the papers rather than the metadata. The database
//! keeps the item; the disk keeps the bytes; the key joins them.
//!
//! The layout is `storage/<key>/<filename>` — one directory per attachment, so
//! two papers may both be `paper.pdf` without a mangled name, and a user
//! looking in the folder sees something recognisable.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use yk_core::{Error, Key, Result};

/// Refuse anything larger. A reference manager stores papers; a request this
/// size is a mistake or an attack, and either way the disk should not fill.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

pub struct Storage {
    root: PathBuf,
}

/// Strip everything that could make a filename escape its directory.
///
/// Attacker-supplied or merely careless, `../../etc/passwd` and `C:\x` must both
/// come out as a plain name; the directory is chosen by us, never by the caller.
pub fn safe_filename(raw: &str) -> String {
    let name: String = Path::new(raw)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
        .collect();

    let trimmed = name.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

/// Guess a filename from a URL, falling back to something plausible.
pub fn filename_from_url(url: &str, content_type: Option<&str>) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let guess = safe_filename(path.rsplit('/').next().unwrap_or_default());

    // A dot is not an extension: arXiv ids look like `2401.12345`, and calling
    // that "extension 12345" would leave the browser unable to open the file.
    let looks_like_extension = guess
        .rsplit_once('.')
        .is_some_and(|(_, ext)| {
            (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphabetic())
        });
    if looks_like_extension && guess != "file" {
        return guess;
    }
    // arXiv abs/pdf URLs commonly end in an id with no extension.
    match content_type {
        Some(t) if t.contains("pdf") => format!("{}.pdf", guess.trim_end_matches('.')),
        _ => guess,
    }
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dir(&self, key: &Key) -> PathBuf {
        self.root.join(key.as_str())
    }

    /// Where an attachment's bytes live.
    pub fn path(&self, key: &Key, filename: &str) -> PathBuf {
        self.dir(key).join(safe_filename(filename))
    }

    /// Write bytes for an attachment, replacing anything already there.
    pub async fn put(&self, key: &Key, filename: &str, bytes: &[u8]) -> Result<PathBuf> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(Error::invalid("file is too large"));
        }
        let dir = self.dir(key);
        tokio::fs::create_dir_all(&dir).await.map_err(io)?;

        let path = dir.join(safe_filename(filename));
        // Write beside the target and rename, so a crash mid-write leaves the
        // previous file intact rather than a truncated one.
        let temp = path.with_extension("part");
        let mut file = tokio::fs::File::create(&temp).await.map_err(io)?;
        file.write_all(bytes).await.map_err(io)?;
        file.sync_all().await.map_err(io)?;
        drop(file);
        tokio::fs::rename(&temp, &path).await.map_err(io)?;
        Ok(path)
    }

    /// Read an attachment's bytes.
    pub async fn get(&self, key: &Key, filename: &str) -> Result<Vec<u8>> {
        let path = self.path(key, filename);
        tokio::fs::read(&path)
            .await
            .map_err(|_| Error::not_found(format!("file for {key}")))
    }

    /// Size on disk, or `None` when there is nothing stored.
    pub async fn size(&self, key: &Key, filename: &str) -> Option<u64> {
        tokio::fs::metadata(self.path(key, filename)).await.ok().map(|m| m.len())
    }

    /// The sizes of a whole page of files, in the order asked.
    ///
    /// One handoff to the blocking pool for the page rather than one per file.
    /// `stat` costs a microsecond or two; the async wrapper around it costs
    /// fifty, and awaiting five hundred of them one after another was most of
    /// the file browser's response time. Missing files report zero — a file the
    /// database believes in and the disk does not is what the browser is for
    /// showing, not an error.
    pub async fn sizes(&self, files: &[(Key, String)]) -> Vec<u64> {
        let paths: Vec<PathBuf> = files.iter().map(|(k, f)| self.path(k, f)).collect();
        let wanted = paths.len();
        tokio::task::spawn_blocking(move || {
            paths
                .iter()
                .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_else(|_| vec![0; wanted])
    }

    /// Give a stored file a different name, in place.
    ///
    /// Refuses to overwrite: two attachments of one paper can render to the
    /// same name, and silently replacing one with the other would lose a file
    /// while reporting success.
    pub async fn rename(&self, key: &Key, from: &str, to: &str) -> Result<()> {
        let source = self.path(key, from);
        let target = self.path(key, to);
        if source == target {
            return Ok(());
        }
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Err(Error::invalid(format!("{to} is already there")));
        }
        tokio::fs::rename(&source, &target).await.map_err(io)
    }

    /// Remove everything belonging to an attachment.
    pub async fn remove(&self, key: &Key) -> Result<()> {
        match tokio::fs::remove_dir_all(self.dir(key)).await {
            Ok(()) => Ok(()),
            // Deleting an item that never had a file is not an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }
}

fn io(e: std::io::Error) -> Error {
    Error::internal(format!("storage: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own.
    ///
    /// Tests run in parallel, and a name built only from the pid and a coarse
    /// clock collides — one test then deletes the directory another is writing
    /// into, which fails as a mystifying "no such file".
    fn temp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let dir = std::env::temp_dir().join(format!(
            "yk-storage-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_filename_cannot_escape_its_directory() {
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("/absolute/path.pdf"), "path.pdf");
        // The drive letter is harmless once the separators are gone; what
        // matters is that nothing can address a directory.
        assert_eq!(safe_filename(r"C:\windows\evil.exe"), "Cwindowsevil.exe");
        assert_eq!(safe_filename(".."), "file");
        assert_eq!(safe_filename("   "), "file");
        assert_eq!(safe_filename(".hidden"), "hidden");
    }

    #[test]
    fn a_long_filename_is_bounded() {
        assert!(safe_filename(&"x".repeat(500)).len() <= 120);
    }

    #[test]
    fn urls_yield_a_plausible_filename() {
        assert_eq!(filename_from_url("https://x.test/a/paper.pdf?v=2", None), "paper.pdf");
        // arXiv links carry the id and no extension.
        assert_eq!(
            filename_from_url("https://arxiv.org/pdf/2401.12345", Some("application/pdf")),
            "2401.12345.pdf"
        );
        assert_eq!(filename_from_url("https://x.test/", Some("application/pdf")), "file.pdf");
        assert_eq!(filename_from_url("https://x.test/notes.tar.gz", None), "notes.tar.gz");
    }

    #[tokio::test]
    async fn round_trips_bytes() {
        let root = temp();
        let storage = Storage::new(&root);
        let key = Key::generate();

        storage.put(&key, "paper.pdf", b"%PDF-1.7").await.unwrap();
        assert_eq!(storage.get(&key, "paper.pdf").await.unwrap(), b"%PDF-1.7");
        assert_eq!(storage.size(&key, "paper.pdf").await, Some(8));

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn a_traversing_name_stays_inside_the_attachment_directory() {
        let root = temp();
        let storage = Storage::new(&root);
        let key = Key::generate();

        storage.put(&key, "../escaped.pdf", b"x").await.unwrap();
        assert!(root.join(key.as_str()).join("escaped.pdf").exists());
        assert!(!root.join("escaped.pdf").exists(), "nothing may be written above the key");

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn removing_what_was_never_stored_is_not_an_error() {
        let root = temp();
        let storage = Storage::new(&root);
        assert!(storage.remove(&Key::generate()).await.is_ok());
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn a_replaced_file_leaves_no_partial_behind() {
        let root = temp();
        let storage = Storage::new(&root);
        let key = Key::generate();

        storage.put(&key, "a.pdf", b"first").await.unwrap();
        storage.put(&key, "a.pdf", b"second").await.unwrap();

        assert_eq!(storage.get(&key, "a.pdf").await.unwrap(), b"second");
        let left: Vec<_> = std::fs::read_dir(root.join(key.as_str()))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec!["a.pdf"], "the temporary file is gone");

        std::fs::remove_dir_all(&root).ok();
    }
}
