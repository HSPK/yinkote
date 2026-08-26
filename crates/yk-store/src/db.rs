//! Connection pool, PRAGMA tuning and forward-only migrations.

use std::path::Path;

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
];

/// Handle to the SQLite database.
///
/// Every public method is async and runs the actual SQLite work on the blocking
/// pool, so a slow query can never stall the async runtime.
#[derive(Clone)]
pub struct Db {
    pool: Pool,
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
                 PRAGMA temp_store = MEMORY;",
            )
        });

        // In-memory databases must use a single connection or each connection
        // gets its own empty database.
        let max = if path.is_some() { num_threads() } else { 1 };
        let pool = r2d2::Pool::builder()
            .max_size(max)
            .build(manager)
            .map_err(|e| Error::storage(format!("pool: {e}")))?;

        let db = Db { pool };
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

    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn()?;
        let current: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql_err)?;

        for (idx, (name, sql)) in MIGRATIONS.iter().enumerate() {
            let target = idx as i64 + 1;
            if current >= target {
                continue;
            }
            tracing::info!(migration = name, "applying migration");
            let tx = conn.transaction().map_err(sql_err)?;
            tx.execute_batch(sql).map_err(|e| {
                Error::storage(format!("migration {name} failed: {e}"))
            })?;
            tx.pragma_update(None, "user_version", target)
                .map_err(sql_err)?;
            tx.commit().map_err(sql_err)?;
        }
        Ok(())
    }

    /// Reclaim space and refresh query-planner statistics.
    pub async fn maintenance(&self) -> Result<()> {
        self.call(|c| {
            c.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(sql_err)
        })
        .await
    }

    /// Fold the write-ahead log back into the database.
    ///
    /// `PASSIVE` never blocks and gives up if readers or writers are active, so
    /// it is safe to run on a timer under load — unlike `TRUNCATE`, which waits.
    pub async fn checkpoint(&self) -> Result<()> {
        self.call(|c| {
            c.execute_batch("PRAGMA wal_checkpoint(PASSIVE);").map_err(sql_err)
        })
        .await
    }
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

    #[test]
    fn migration_is_idempotent() {
        let db = Db::open(None).unwrap();
        db.migrate().unwrap();
        db.migrate().unwrap();
    }
}
