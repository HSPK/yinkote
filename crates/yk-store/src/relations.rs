//! Citations: the edges the library cannot work out for itself.
//!
//! Everything else the graph draws is derived from the items, so it is queried
//! rather than stored (see `graph`). A citation is the opposite: it comes from
//! the publisher, it is about a work that is usually not in the library at all,
//! and if it is not written down it is gone.
//!
//! A cited work is addressed by fingerprint rather than by id, and resolved
//! when the graph is read. That is what lets a reference to a paper acquired
//! later become an ordinary edge the moment it arrives, with no backfill and no
//! window in which the library holds both papers and still draws them as
//! strangers.

use async_trait::async_trait;
use rusqlite::params;
use serde::Serialize;
use yk_core::{Error, Key, Result};

use crate::db::{sql_err, write_tx, Db};

/// The kind of relation this module stores. One, for now, and named rather
/// than assumed so the second one does not have to migrate the first.
pub const CITES: &str = "cites";

/// One work cited by another.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Citation {
    /// Where it sits in the source's bibliography.
    pub position: i64,
    /// The item in this library, when there is one.
    pub key: Option<Key>,
    /// What to call it when there is not.
    pub label: String,
    pub year: Option<i64>,
    /// `doi:…`, or empty when the publisher deposited no identifier.
    pub fingerprint: String,
}

/// What to store for one reference.
#[derive(Debug, Clone)]
pub struct CitationDraft {
    pub fingerprint: String,
    pub label: String,
    pub year: Option<i64>,
}

#[async_trait]
pub trait RelationRepository: Send + Sync {
    /// Replace an item's reference list wholesale.
    ///
    /// Wholesale because a bibliography is a single thing that a publisher
    /// either deposited or did not; merging two versions of one would leave a
    /// list that matches no printed paper.
    async fn set_citations(
        &self,
        library_id: i64,
        key: &Key,
        citations: Vec<CitationDraft>,
    ) -> Result<u64>;

    /// What this item cites, in the order it cites them.
    async fn cites(&self, library_id: i64, key: &Key) -> Result<Vec<Citation>>;

    /// What in this library cites this item.
    async fn cited_by(&self, library_id: i64, key: &Key) -> Result<Vec<Citation>>;
}

#[derive(Clone)]
pub struct SqliteRelationRepository {
    db: Db,
}

impl SqliteRelationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn id_of(conn: &rusqlite::Connection, library_id: i64, key: &Key) -> Result<i64> {
    conn.query_row(
        "SELECT id FROM items WHERE library_id=?1 AND key=?2",
        params![library_id, key.as_str()],
        |r| r.get(0),
    )
    .map_err(|_| Error::not_found(format!("item {key}")))
}

#[async_trait]
impl RelationRepository for SqliteRelationRepository {
    async fn set_citations(
        &self,
        library_id: i64,
        key: &Key,
        citations: Vec<CitationDraft>,
    ) -> Result<u64> {
        let db = self.db.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.conn()?;
            let id = id_of(&conn, library_id, &key)?;
            let tx = write_tx(&mut conn)?;

            tx.execute(
                "DELETE FROM item_relations WHERE source_id=?1 AND kind=?2",
                params![id, CITES],
            )
            .map_err(sql_err)?;

            let mut stored = 0u64;
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT INTO item_relations
                         (source_id, kind, position, target_key, target_label, target_year)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )
                    .map_err(sql_err)?;
                for (position, c) in citations.iter().enumerate() {
                    stmt.execute(params![
                        id,
                        CITES,
                        position as i64,
                        c.fingerprint,
                        c.label,
                        c.year
                    ])
                    .map_err(sql_err)?;
                    stored += 1;
                }
            }
            tx.commit().map_err(sql_err)?;
            Ok(stored)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn cites(&self, library_id: i64, key: &Key) -> Result<Vec<Citation>> {
        let db = self.db.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            let id = id_of(&conn, library_id, &key)?;

            // The left join is the resolution: a cited work that is in the
            // library comes back with its key, one that is not comes back with
            // the label the publisher gave it. Neither case is special.
            let mut stmt = conn
                .prepare_cached(
                    "SELECT r.position, r.target_key, r.target_label, r.target_year, i.key
                     FROM item_relations r
                     LEFT JOIN items i
                          ON i.library_id = ?1 AND i.deleted = 0
                         AND r.target_key != '' AND i.fingerprint = r.target_key
                     WHERE r.source_id = ?2 AND r.kind = ?3
                     ORDER BY r.position",
                )
                .map_err(sql_err)?;

            let rows = stmt
                .query_map(params![library_id, id, CITES], map_citation)
                .map_err(sql_err)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn cited_by(&self, library_id: i64, key: &Key) -> Result<Vec<Citation>> {
        let db = self.db.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            let fingerprint: String = conn
                .query_row(
                    "SELECT fingerprint FROM items WHERE library_id=?1 AND key=?2",
                    params![library_id, key.as_str()],
                    |r| r.get(0),
                )
                .map_err(|_| Error::not_found(format!("item {key}")))?;

            // An item with no identifier of its own cannot be recognised in
            // anybody's bibliography, and saying "nothing cites it" is more
            // honest than a title match that would be wrong sometimes.
            if !fingerprint.starts_with("doi:") {
                return Ok(Vec::new());
            }

            let mut stmt = conn
                .prepare_cached(
                    "SELECT r.position, r.target_key, i.fields, i.year, i.key
                     FROM item_relations r
                     CROSS JOIN items i ON i.id = r.source_id
                     WHERE r.target_key = ?1 AND r.kind = ?2
                       AND i.library_id = ?3 AND i.deleted = 0
                     ORDER BY i.year DESC",
                )
                .map_err(sql_err)?;

            let rows = stmt
                .query_map(params![fingerprint, CITES, library_id], |r| {
                    let fields: String = r.get(2)?;
                    Ok(Citation {
                        position: r.get(0)?,
                        fingerprint: r.get(1)?,
                        label: title_of(&fields),
                        year: r.get(3)?,
                        key: r
                            .get::<_, String>(4)
                            .ok()
                            .and_then(|k| Key::parse(&k).ok()),
                    })
                })
                .map_err(sql_err)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }
}

fn map_citation(r: &rusqlite::Row<'_>) -> rusqlite::Result<Citation> {
    Ok(Citation {
        position: r.get(0)?,
        fingerprint: r.get(1)?,
        label: r.get(2)?,
        year: r.get(3)?,
        key: r.get::<_, Option<String>>(4)?.and_then(|k| Key::parse(&k).ok()),
    })
}

fn title_of(fields: &str) -> String {
    serde_json::from_str::<serde_json::Value>(fields)
        .ok()
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
