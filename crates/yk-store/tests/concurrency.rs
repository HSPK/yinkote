//! Concurrent writers must queue behind each other, not fail.
//!
//! This reproduces a real bug: with deferred transactions SQLite returns
//! `SQLITE_BUSY` immediately when it cannot upgrade a read lock, so the busy
//! timeout never gets a chance to help.

use std::sync::Arc;

use yk_core::model::ItemDraft;
use yk_core::query::ItemFilter;
use yk_store::Store;

/// A directory of this test's own, cleaned up when the test ends.
///
/// Tests run in parallel, so a name built from the pid alone collides and one
/// test deletes the files another is using.
struct Root(std::path::PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let dir = std::env::temp_dir().join(format!(
            "yk-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn db(&self) -> std::path::PathBuf {
        self.0.join("test.db")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

const WORKERS: i64 = 8;
const BATCHES: i64 = 5;
const PER_BATCH: i64 = 50;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn parallel_batch_writes_do_not_fail() {
    let dir = std::env::temp_dir().join(format!("yk-conc-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();

    let store = Store::open(Some(&dir.join("test.db"))).unwrap();
    let lib = store.default_library;

    let busy: i64 = store
        .db()
        .call(|c| c.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).map_err(yk_store::sql_err))
        .await
        .unwrap();
    assert!(busy >= 1000, "busy_timeout not applied to pooled connections: {busy}");

    let store = Arc::new(store);
    let mut handles = Vec::new();
    for w in 0..WORKERS {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            for b in 0..BATCHES {
                let drafts: Vec<ItemDraft> = (0..PER_BATCH)
                    .map(|i| {
                        ItemDraft::new("journalArticle")
                            .with_field("title", format!("w{w}-b{b}-{i}"))
                    })
                    .collect();
                store.items.create_many(lib, drafts).await.expect("batch write");
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let total =
        store.items.count(&ItemFilter { library_id: lib, ..Default::default() }).await.unwrap();
    assert_eq!(total, WORKERS * BATCHES * PER_BATCH);

    std::fs::remove_dir_all(&dir).ok();
}

/// The write-ahead log must not grow without bound under sustained writes.
///
/// This is the regression that a passive-only checkpoint could not prevent: a
/// 246 MB library was observed with a 1 GB log, because a passive pass can only
/// reclaim frames older than the oldest live reader and a busy pool always has
/// one. Nothing about the data is wrong when this happens, so only a size
/// assertion catches it.
#[tokio::test]
async fn checkpointing_bounds_the_write_ahead_log() {
    let root = Root::new("wal-bound");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    // Enough writes to push the log well past the truncate threshold used here.
    for batch in 0..20 {
        let drafts: Vec<_> = (0..200)
            .map(|i| {
                ItemDraft::new("journalArticle")
                    .with_field("title", format!("Item {batch}-{i}"))
                    .with_field("abstractNote", "x".repeat(2_000))
            })
            .collect();
        store.items.create_many(lib, drafts).await.unwrap();
    }

    let before = wal_bytes(&root.db());
    assert!(before > 0, "the test needs a log to reclaim; got {before} bytes");

    // Folding alone leaves the file at its high-water mark, so a generous
    // threshold must *not* shrink it — this is the case that made an earlier
    // frame-count check useless.
    let folded = store.db().checkpoint(u64::MAX).await.unwrap();
    assert_eq!(folded, wal_bytes(&root.db()));
    assert!(folded >= before, "folding reuses the space rather than releasing it");

    // A threshold below the current size forces the escalation the worker takes.
    let after = store.db().checkpoint(1024).await.unwrap();
    assert_eq!(after, 0, "a truncating checkpoint resets the file");
    assert_eq!(wal_bytes(&root.db()), 0);
}

fn wal_bytes(db: &std::path::Path) -> u64 {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    std::fs::metadata(std::path::PathBuf::from(wal)).map(|m| m.len()).unwrap_or(0)
}

/// A backup is worth exactly what can be restored from it.
///
/// Taking one is easy to get wrong in a way that still produces a file: a copy
/// made without a consistent snapshot opens fine and is missing rows. So the
/// test opens the copy as a library in its own right and reads it.
#[tokio::test]
async fn a_backup_opens_as_a_library() {
    let root = Root::new("backup");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    let drafts: Vec<ItemDraft> = (0..200)
        .map(|i| ItemDraft::new("journalArticle").with_field("title", format!("Paper {i}")))
        .collect();
    store.items.create_many(lib, drafts).await.unwrap();

    let copy = root.0.join("backup.db");
    let bytes = store.db().backup_to(copy.clone()).await.unwrap();
    assert!(bytes > 0, "a backup with nothing in it is not a backup");

    // Writing afterwards must not reach into the copy: what was backed up is
    // what the library held at the time.
    store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Written later"))
        .await
        .unwrap();

    let restored = Store::open(Some(&copy)).unwrap();
    let found = restored
        .items
        .count(&ItemFilter { library_id: lib, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(found, 200, "every row, and only the rows that existed");

    // Refusing to overwrite is the difference between a backup and a mistake.
    assert!(store.db().backup_to(copy).await.is_err());
}
