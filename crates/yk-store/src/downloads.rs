//! The download queue.
//!
//! Fetching a file is slow, fails often and is worth retrying — three
//! properties that make it a queue rather than a request. The click that asks
//! for a paper should return immediately, a publisher that times out at three
//! in the morning should still be retryable in the morning, and neither should
//! depend on a browser tab still being open.
//!
//! Kept in the database for the same reason: a queue that forgets everything
//! when the process restarts is not a queue, it is a buffer.

use async_trait::async_trait;
use rusqlite::params;
use serde::Serialize;
use yk_core::{now_ms, Error, Result};

use crate::db::{sql_err, write_tx, Db};

/// What a download is doing.
pub mod state {
    pub const WAITING: &str = "waiting";
    pub const RUNNING: &str = "running";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
}

/// One file the library is waiting for.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: i64,
    pub item_key: String,
    pub url: String,
    pub state: String,
    pub attempts: i64,
    /// Why it failed, kept rather than logged: it is what the user needs in
    /// order to judge whether retrying is worth anything.
    pub error: String,
    pub title: String,
    pub bytes: i64,
    pub updated_at: i64,
}

/// What to add.
#[derive(Debug, Clone)]
pub struct DownloadDraft {
    pub item_key: String,
    pub url: String,
    pub title: String,
}

#[async_trait]
pub trait DownloadQueue: Send + Sync {
    /// Add files to fetch. Returns how many were new.
    ///
    /// Asking twice for the same file is the same request, not two — the
    /// commonest way it happens is a double-clicked button.
    async fn enqueue(&self, library_id: i64, drafts: Vec<DownloadDraft>) -> Result<u64>;

    /// Take the next waiting download, marking it running.
    ///
    /// Claiming and marking happen in one transaction, so two workers cannot
    /// take the same row — the queue is the only thing that knows what is
    /// already in flight.
    async fn claim(&self, library_id: i64) -> Result<Option<Download>>;

    async fn succeed(&self, id: i64, bytes: i64) -> Result<()>;
    async fn fail(&self, id: i64, error: &str) -> Result<()>;

    /// Put a failed download back in the queue.
    async fn retry(&self, library_id: i64, ids: &[i64]) -> Result<u64>;
    async fn remove(&self, library_id: i64, ids: &[i64]) -> Result<u64>;
    /// Forget everything that finished, leaving what still needs attention.
    async fn clear_finished(&self, library_id: i64) -> Result<u64>;

    async fn list(&self, library_id: i64, limit: u32) -> Result<Vec<Download>>;
}

#[derive(Clone)]
pub struct SqliteDownloadQueue {
    db: Db,
}

impl SqliteDownloadQueue {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

const SELECT: &str = "SELECT id, item_key, url, state, attempts, error, title, bytes, updated_at \
                      FROM fetch_queue";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<Download> {
    Ok(Download {
        id: r.get(0)?,
        item_key: r.get(1)?,
        url: r.get(2)?,
        state: r.get(3)?,
        attempts: r.get(4)?,
        error: r.get(5)?,
        title: r.get(6)?,
        bytes: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

#[async_trait]
impl DownloadQueue for SqliteDownloadQueue {
    async fn enqueue(&self, library_id: i64, drafts: Vec<DownloadDraft>) -> Result<u64> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.conn()?;
            let tx = write_tx(&mut conn)?;
            let now = now_ms();
            let mut added = 0u64;

            {
                let mut stmt = tx
                    .prepare(
                        "INSERT INTO fetch_queue
                         (library_id, item_key, url, title, state, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                         ON CONFLICT(library_id, item_key, url) DO UPDATE SET
                            -- Asking again for something that failed is a
                            -- retry, which is what the user means by it.
                            state = CASE WHEN fetch_queue.state = ?7
                                         THEN ?5 ELSE fetch_queue.state END,
                            updated_at = ?6",
                    )
                    .map_err(sql_err)?;

                for draft in &drafts {
                    let url = draft.url.trim();
                    if url.is_empty() {
                        continue;
                    }
                    added += stmt
                        .execute(params![
                            library_id,
                            draft.item_key,
                            url,
                            draft.title,
                            state::WAITING,
                            now,
                            state::FAILED
                        ])
                        .map_err(sql_err)? as u64;
                }
            }

            tx.commit().map_err(sql_err)?;
            Ok(added)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn claim(&self, library_id: i64) -> Result<Option<Download>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.conn()?;
            let tx = write_tx(&mut conn)?;

            let found: Option<Download> = tx
                .prepare_cached(&format!(
                    "{SELECT} WHERE library_id = ?1 AND state = ?2 ORDER BY id LIMIT 1"
                ))
                .map_err(sql_err)?
                .query_map(params![library_id, state::WAITING], map)
                .map_err(sql_err)?
                .next()
                .transpose()
                .map_err(sql_err)?;

            let Some(mut download) = found else {
                return Ok(None);
            };

            tx.execute(
                "UPDATE fetch_queue SET state = ?1, attempts = attempts + 1, updated_at = ?2 \
                 WHERE id = ?3",
                params![state::RUNNING, now_ms(), download.id],
            )
            .map_err(sql_err)?;
            tx.commit().map_err(sql_err)?;

            download.state = state::RUNNING.into();
            download.attempts += 1;
            Ok(Some(download))
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn succeed(&self, id: i64, bytes: i64) -> Result<()> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            conn.execute(
                "UPDATE fetch_queue SET state = ?1, error = '', bytes = ?2, updated_at = ?3 \
                 WHERE id = ?4",
                params![state::DONE, bytes, now_ms(), id],
            )
            .map_err(sql_err)?;
            Ok(())
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn fail(&self, id: i64, error: &str) -> Result<()> {
        let db = self.db.clone();
        let error = error.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            conn.execute(
                "UPDATE fetch_queue SET state = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
                params![state::FAILED, error, now_ms(), id],
            )
            .map_err(sql_err)?;
            Ok(())
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn retry(&self, library_id: i64, ids: &[i64]) -> Result<u64> {
        self.set_state(library_id, ids, state::WAITING).await
    }

    async fn remove(&self, library_id: i64, ids: &[i64]) -> Result<u64> {
        let db = self.db.clone();
        let ids = ids.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.conn()?;
            let tx = write_tx(&mut conn)?;
            let mut removed = 0u64;
            for id in ids {
                removed += tx
                    .execute(
                        "DELETE FROM fetch_queue WHERE library_id = ?1 AND id = ?2",
                        params![library_id, id],
                    )
                    .map_err(sql_err)? as u64;
            }
            tx.commit().map_err(sql_err)?;
            Ok(removed)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    /// Both terminal states, not just the successful one.
    ///
    /// This deleted only `done`, so on the queue that actually needs tidying --
    /// one full of failures -- the button labelled "clear finished" reported
    /// success and removed nothing. A failed download *is* finished: nothing
    /// will move it again without being asked. Retrying is still available
    /// before the list is cleared, and single rows have `remove`.
    async fn clear_finished(&self, library_id: i64) -> Result<u64> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            let n = conn
                .execute(
                    "DELETE FROM fetch_queue WHERE library_id = ?1 AND state IN (?2, ?3)",
                    params![library_id, state::DONE, state::FAILED],
                )
                .map_err(sql_err)?;
            Ok(n as u64)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn list(&self, library_id: i64, limit: u32) -> Result<Vec<Download>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            let mut stmt = conn
                .prepare_cached(&format!(
                    // Unfinished first: the rows that need a decision are the
                    // ones worth putting at the top.
                    "{SELECT} WHERE library_id = ?1
                     ORDER BY CASE state WHEN 'failed' THEN 0 WHEN 'running' THEN 1
                                         WHEN 'waiting' THEN 2 ELSE 3 END, id DESC
                     LIMIT ?2"
                ))
                .map_err(sql_err)?;
            let rows = stmt.query_map(params![library_id, limit], map).map_err(sql_err)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }
}

impl SqliteDownloadQueue {
    async fn set_state(&self, library_id: i64, ids: &[i64], next: &str) -> Result<u64> {
        let db = self.db.clone();
        let ids = ids.to_vec();
        let next = next.to_string();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.conn()?;
            let tx = write_tx(&mut conn)?;
            let now = now_ms();
            let mut changed = 0u64;
            for id in ids {
                changed += tx
                    .execute(
                        "UPDATE fetch_queue SET state = ?1, error = '', updated_at = ?2 \
                         WHERE library_id = ?3 AND id = ?4",
                        params![next, now, library_id, id],
                    )
                    .map_err(sql_err)? as u64;
            }
            tx.commit().map_err(sql_err)?;
            Ok(changed)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }
}

#[cfg(test)]
mod tests;
