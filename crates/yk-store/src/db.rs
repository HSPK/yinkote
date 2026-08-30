//! Connection pool, PRAGMA tuning and forward-only migrations.

use std::path::{Path, PathBuf};

use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use yk_core::{Error, Result};

pub type Pool = r2d2::Pool<SqliteConnectionManager>;
pub type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

/// Ordered, forward-only migrations. The index in this slice *is* the target
/// `user_version`, so never reorder or remove an entry.
const MIGRATIONS: &[(&str, &str)] = &[
    ("001_init", include_str!("../migrations/001_init.sql")),
    ("002_smart_collections", include_str!("../migrations/002_smart_collections.sql")),
    ("003_conversations", include_str!("../migrations/003_conversations.sql")),
    (
        "004_collection_appearance",
        include_str!("../migrations/004_collection_appearance.sql"),
    ),
    ("005_relations", include_str!("../migrations/005_relations.sql")),
    ("006_relation_doi", include_str!("../migrations/006_relation_doi.sql")),
    ("007_citation_fetches", include_str!("../migrations/007_citation_fetches.sql")),
    ("008_fetch_queue", include_str!("../migrations/008_fetch_queue.sql")),
    ("009_cited_works", include_str!("../migrations/009_cited_works.sql")),
    ("010_message_mentions", include_str!("../migrations/010_message_mentions.sql")),
    ("011_sort_tiebreak", include_str!("../migrations/011_sort_tiebreak.sql")),
    ("012_attachment_rank", include_str!("../migrations/012_attachment_rank.sql")),
    ("013_attachment_browse", include_str!("../migrations/013_attachment_browse.sql")),
    ("014_ranked_search_join", include_str!("../migrations/014_ranked_search_join.sql")),
    ("015_collection_dates", include_str!("../migrations/015_collection_dates.sql")),
];

/// Handle to the SQLite database.
///
/// Every public method is async and runs the actual SQLite work on the blocking
/// pool, so a slow query can never stall the async runtime.
#[derive(Clone)]
pub struct Db {
    pool: Pool,
    /// Kept so the size of the write-ahead log can be read from disk. SQLite
    /// reports how many *frames* the log holds, which says nothing about how
    /// large the file has grown — see [`Db::checkpoint`].
    path: Option<PathBuf>,
}

/// How many items a database file holds, without opening it as a library.
///
/// For describing a snapshot that has just been written: counting the *live*
/// library instead answers a different question, because it has moved on since
/// the snapshot was taken, and a manifest that disagrees with the database it
/// travels with is worse than no manifest.
///
/// Lives here because this is the crate that owns SQLite; the server reaching
/// for `rusqlite` directly would put database handling in two places.
pub fn item_count_of(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT count(*) FROM items", [], |r| r.get(0)))
        .unwrap_or(0)
}

impl Db {
    /// Open (and migrate) a database file. Pass `None` for an in-memory
    /// database, which is what tests use.
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let manager = match path {
            Some(p) => {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                SqliteConnectionManager::file(p)
            }
            // A shared in-memory database so every pooled connection sees the
            // same data.
            None => SqliteConnectionManager::memory().with_flags(
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                    | rusqlite::OpenFlags::SQLITE_OPEN_URI
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
        };

        // Switching to WAL needs a brief exclusive lock. Do it once, up front,
        // on a dedicated connection — pool connections initialise concurrently
        // and would otherwise race and log spurious "database is locked".
        if let Some(p) = path {
            let conn = rusqlite::Connection::open(p).map_err(sql_err)?;
            conn.busy_timeout(std::time::Duration::from_secs(15)).map_err(sql_err)?;
            conn.pragma_update(None, "journal_mode", "WAL").map_err(sql_err)?;
        }

        let manager = manager.with_init(|c| {
            c.execute_batch(
                "PRAGMA busy_timeout = 15000;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA cache_size = -65536;   -- 64 MiB page cache
                 PRAGMA mmap_size = 268435456; -- 256 MiB
                 PRAGMA temp_store = MEMORY;
                 -- Checkpointing belongs to the background worker, not to
                 -- whichever write happens to cross the threshold. SQLite's
                 -- default makes one unlucky writer in a few dozen copy the
                 -- whole log into the database mid-transaction: 273 ms of
                 -- commit behind 142 microseconds of actual work, a p95 a
                 -- hundred times the p50. See `checkpoint`, and the worker
                 -- that calls it.
                 PRAGMA wal_autocheckpoint = 0;",
            )
        });

        // In-memory databases must use a single connection or each connection
        // gets its own empty database.
        let max = if path.is_some() { num_threads() } else { 1 };
        let pool = r2d2::Pool::builder()
            .max_size(max)
            .build(manager)
            .map_err(|e| Error::storage(format!("pool: {e}")))?;

        let db = Db { pool, path: path.map(Path::to_path_buf) };
        db.migrate()?;
        Ok(db)
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn conn(&self) -> Result<PooledConn> {
        self.pool
            .get()
            .map_err(|e| Error::storage(format!("checkout: {e}")))
    }

    /// Run a closure with a pooled connection on the blocking thread pool.
    pub async fn call<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| Error::storage(format!("checkout: {e}")))?;
            f(&mut conn)
        })
        .await
        .map_err(|e| Error::internal(format!("join: {e}")))?
    }

    /// Bring the schema up to date.
    ///
    /// Applied **by name**, not by position. They used to be applied by
    /// position, with the names logged and never checked — which means removing
    /// or reordering one silently skips whatever takes its place. That is not
    /// hypothetical: a migration added and then reverted during development
    /// consumed its slot, and the next migration to take that number was never
    /// applied to any database that had seen the first. The failure surfaced
    /// two features later as a missing table.
    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                 name       TEXT PRIMARY KEY,
                 applied_at INTEGER NOT NULL
             )",
        )
        .map_err(sql_err)?;

        self.backfill_names(&conn)?;

        let applied: std::collections::HashSet<String> = {
            let mut stmt =
                conn.prepare("SELECT name FROM schema_migrations").map_err(sql_err)?;
            let names = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(sql_err)?;
            names.filter_map(|n| n.ok()).collect()
        };

        for (idx, (name, sql)) in MIGRATIONS.iter().enumerate() {
            if applied.contains(*name) {
                continue;
            }
            tracing::info!(migration = name, "applying migration");
            let tx = conn.transaction().map_err(sql_err)?;
            tx.execute_batch(sql)
                .map_err(|e| Error::storage(format!("migration {name} failed: {e}")))?;
            tx.execute(
                "INSERT OR REPLACE INTO schema_migrations (name, applied_at) VALUES (?1, ?2)",
                rusqlite::params![name, yk_core::now_ms()],
            )
            .map_err(sql_err)?;
            // Kept in step so anything reading the pragma — including a person
            // with a SQLite shell — still sees how far the schema has come.
            tx.pragma_update(None, "user_version", idx as i64 + 1).map_err(sql_err)?;
            tx.commit().map_err(sql_err)?;
        }
        Ok(())
    }

    /// Give names to migrations that were applied before names were recorded.
    ///
    /// A database from before this table can only say *how many* ran, so the
    /// first that many are assumed — there is nothing else to go on. It is done
    /// once, and from then on the record is exact.
    fn backfill_names(&self, conn: &Connection) -> Result<()> {
        let known: i64 = conn
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .map_err(sql_err)?;
        if known > 0 {
            return Ok(());
        }

        let version: i64 =
            conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(sql_err)?;
        if version <= 0 {
            return Ok(());
        }

        for (name, _) in MIGRATIONS.iter().take(version as usize) {
            conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (name, applied_at) VALUES (?1, ?2)",
                rusqlite::params![name, yk_core::now_ms()],
            )
            .map_err(sql_err)?;
        }
        Ok(())
    }

    /// Reclaim space and refresh query-planner statistics.
    ///
    /// Both halves at once, for the explicit "optimise now" endpoint. The
    /// background worker uses [`Db::refresh_statistics`] instead, because the
    /// checkpoint half has a policy of its own about when it may run.
    pub async fn maintenance(&self) -> Result<()> {
        self.call(|c| {
            c.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(sql_err)
        })
        .await
    }

    /// Refresh query-planner statistics, and nothing else.
    ///
    /// Deliberately without the checkpoint. A `TRUNCATE` checkpoint takes the
    /// database exclusively, which is why `checkpoint_worker` refuses to run
    /// one while a bulk write is in progress; a second caller doing it on a
    /// timer would walk straight into the case that worker exists to avoid.
    pub async fn refresh_statistics(&self) -> Result<()> {
        self.call(|c| c.execute_batch("PRAGMA optimize;").map_err(sql_err)).await
    }

    /// Merge some of the full-text index's accumulated segments.
    ///
    /// Every write appends a segment; nothing ever merged them, so the search
    /// index degrades for as long as the library is used. Measured on a real
    /// 100,000-item library after a few weeks of writes: **30,770 segments**,
    /// and merging them took a common keyword search from 30.6ms to 23.9ms.
    /// A library nobody ever reindexes only gets slower.
    ///
    /// `PRAGMA optimize` does not do this. It refreshes the *query planner's*
    /// statistics, which is a different thing with a confusingly similar name,
    /// and it was the only maintenance this database had.
    ///
    /// Bounded on purpose: `'merge', N` does about N pages of work and stops,
    /// where a full `'optimize'` on that library took 41 seconds and holds the
    /// write lock throughout. This runs on a timer beside the statistics, so
    /// it has to be something nobody notices; being idempotent, it costs
    /// nothing at all once there is little left to merge.
    pub async fn merge_search_segments(&self) -> Result<()> {
        self.call(|c| {
            c.execute_batch(
                "INSERT INTO items_fts(items_fts, rank) VALUES('merge', 64);
                 INSERT INTO items_trgm(items_trgm, rank) VALUES('merge', 64);",
            )
            .map_err(sql_err)
        })
        .await
    }

    /// Write a consistent copy of the database to `dest`.
    ///
    /// `VACUUM INTO` rather than the backup API's page-by-page copy: it takes
    /// the same read snapshot, so a backup taken while somebody is typing is
    /// still a coherent library, and it writes a *compacted* file — which for a
    /// database that has had a large import deleted out of it can be a third of
    /// the size. It also cannot half-finish: SQLite refuses if the destination
    /// exists, and a failure leaves nothing behind to be mistaken for a backup.
    ///
    /// Returns the size of what was written.
    pub async fn backup_to(&self, dest: std::path::PathBuf) -> Result<u64> {
        if dest.exists() {
            return Err(yk_core::Error::invalid(format!(
                "{} already exists",
                dest.display()
            )));
        }
        let target = dest.clone();
        self.call(move |c| {
            // Bound as a parameter would be ideal; `VACUUM INTO` will not take
            // one, so the path is quoted the way SQLite quotes strings.
            let quoted = target.to_string_lossy().replace('\'', "''");
            c.execute_batch(&format!("VACUUM INTO '{quoted}'")).map_err(sql_err)
        })
        .await?;
        Ok(std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0))
    }

    /// Fold the write-ahead log back into the database, and keep its *file*
    /// from staying huge. Returns the log's size in bytes afterwards.
    ///
    /// Two separate things go wrong here and only one of them is obvious.
    ///
    /// `PASSIVE` never blocks, which is what makes it safe on a timer, but it
    /// only reclaims frames older than the oldest live reader — and a busy pool
    /// always has one. Under a burst of concurrent writers the log outruns it
    /// and grows: a 246 MB library was observed with a 1 GB log.
    ///
    /// The subtler part is that folding the log does not shrink the file.
    /// SQLite keeps the space and reuses it, so after a successful pass the
    /// pragma reports a few hundred frames while a gigabyte sits on disk.
    /// Deciding from the frame count therefore never escalates, which is
    /// exactly the bug this once had. The file's size on disk is the thing that
    /// matters, so that is what is measured.
    ///
    /// `TRUNCATE` waits for readers and then resets the file. That pause is
    /// real and much cheaper than an unbounded log, which costs disk and slows
    /// every read, since each one searches the log's index first.
    /// Fold the write-ahead log back into the database.
    ///
    /// **Somebody must call this.** Automatic checkpointing is switched off in
    /// every connection (see the pragmas above), so a `Store` with nobody
    /// running this will grow its log without bound. The server has a worker;
    /// anything else embedding the store owes it the same.
    pub async fn checkpoint(&self, truncate_above_bytes: u64) -> Result<u64> {
        let wal = self.wal_path();
        self.call(move |c| {
            checkpoint_once(c, "PASSIVE")?;
            let size = wal.as_deref().map(wal_bytes).unwrap_or(0);
            if size <= truncate_above_bytes {
                return Ok(size);
            }
            tracing::debug!(bytes = size, "write-ahead log is large; truncating");
            checkpoint_once(c, "TRUNCATE")?;
            Ok(wal.as_deref().map(wal_bytes).unwrap_or(0))
        })
        .await
    }

    /// Where the write-ahead log lives, for in-memory databases `None`.
    fn wal_path(&self) -> Option<PathBuf> {
        self.path.as_ref().map(|p| {
            let mut name = p.clone().into_os_string();
            name.push("-wal");
            PathBuf::from(name)
        })
    }
}

/// Run one checkpoint and report the frames still in the log.
///
/// The pragma answers `(busy, log_frames, checkpointed_frames)`; a busy result
/// is normal rather than an error, so the caller carries on either way.
fn checkpoint_once(c: &Connection, mode: &str) -> Result<i64> {
    c.query_row(&format!("PRAGMA wal_checkpoint({mode})"), [], |r| r.get::<_, i64>(1))
        .map_err(sql_err)
}

fn wal_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Begin a write transaction.
///
/// Always `BEGIN IMMEDIATE`: a deferred transaction takes a read lock first and
/// SQLite refuses to upgrade it once another writer has committed, returning
/// `SQLITE_BUSY` *immediately* — the busy timeout cannot help. Taking the write
/// lock up front makes concurrent writers queue politely instead.
pub fn write_tx(conn: &mut Connection) -> Result<rusqlite::Transaction<'_>> {
    conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_err)
}

fn num_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() as u32).clamp(4, 16))
        .unwrap_or(8)
}

/// Map a rusqlite error onto the domain error taxonomy.
///
/// The extended result code is kept in the message: "database is locked" alone
/// is not enough to tell a lock-upgrade failure from a checkpoint starvation.
pub fn sql_err(e: rusqlite::Error) -> Error {
    use rusqlite::Error as E;
    match &e {
        E::QueryReturnedNoRows => Error::NotFound("row".into()),
        E::SqliteFailure(f, msg) => {
            let msg = msg.clone().unwrap_or_default();
            match f.code {
                rusqlite::ErrorCode::ConstraintViolation => Error::Conflict(msg),
                _ => Error::Storage(format!("{msg} (sqlite {:?}/{})", f.code, f.extended_code)),
            }
        }
        _ => Error::Storage(format!("{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_in_memory() {
        let db = Db::open(None).unwrap();
        let conn = db.conn().unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='items'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn refreshing_statistics_gives_the_planner_something_to_plan_with() {
        // Nothing in the program ran `PRAGMA optimize` except a hand-written
        // request, so a real library had no `sqlite_stat1` at all and SQLite
        // guessed. Listing a shelf cost 14.2ms guessed against 4.4ms informed.
        let store = crate::Store::in_memory().unwrap();
        let lib = store.default_library;
        for i in 0..400 {
            let draft = yk_core::model::ItemDraft::new("journalArticle")
                .with_field("title", format!("Paper {i}"));
            store.items.create(lib, draft).await.unwrap();
        }

        store.db().refresh_statistics().await.unwrap();

        let conn = store.db().conn().unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_stat1", [], |r| r.get(0))
            .unwrap_or(0);
        assert!(rows > 0, "analysed nothing, so the planner is still guessing");
    }

    #[test]
    fn no_connection_checkpoints_in_the_foreground() {
        // SQLite's default makes one unlucky writer in a few dozen copy the
        // whole log into the database mid-commit: 273 ms behind 142 µs of real
        // work, a p95 a hundred times the p50. The background worker exists to
        // do that instead, and this pragma is the only thing that stops both
        // happening. It is one line, it is invisible, and it will be tidied
        // away by somebody who does not know what it costs.
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(Some(&dir.path().join("test.db"))).unwrap();
        let conn = db.conn().unwrap();

        let pages: i64 = conn.query_row("PRAGMA wal_autocheckpoint", [], |r| r.get(0)).unwrap();
        assert_eq!(pages, 0, "checkpointing belongs to the worker, not to a write");
    }

    #[test]
    fn records_which_migrations_ran_by_name() {
        let db = Db::open(None).unwrap();
        let conn = db.conn().unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT name FROM schema_migrations ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|n| n.ok())
            .collect();

        assert_eq!(names.len(), MIGRATIONS.len());
        assert!(names.contains(&"001_init".to_string()));
    }

    #[test]
    fn an_unknown_record_does_not_derail_the_ones_we_know() {
        // A slot consumed by a migration that no longer exists is exactly the
        // situation that caused this: one added and reverted during
        // development took number five, and the feature that later took that
        // number was never applied to any database that had seen the first.
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(Some(&dir.path().join("test.db"))).unwrap();
        {
            let conn = db.conn().unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (name, applied_at) VALUES ('005_a_reverted_idea', 0)",
                [],
            )
            .unwrap();
        }

        db.migrate().unwrap();

        let conn = db.conn().unwrap();
        for (name, _) in MIGRATIONS {
            let seen: i64 = conn
                .query_row(
                    "SELECT count(*) FROM schema_migrations WHERE name = ?1",
                    [name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(seen, 1, "{name} should be recorded exactly once");
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let db = Db::open(None).unwrap();
        db.migrate().unwrap();
        db.migrate().unwrap();
    }
}
