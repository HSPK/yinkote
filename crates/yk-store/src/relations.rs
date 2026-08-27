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
    /// The DOI as deposited. The fingerprint cannot be turned back into one.
    pub doi: String,
}

/// What to store for one reference.
#[derive(Debug, Clone)]
pub struct CitationDraft {
    pub fingerprint: String,
    /// The DOI as deposited, kept because the fingerprint is one-way.
    pub doi: String,
    pub label: String,
    pub year: Option<i64>,
}

/// A work the library keeps citing and does not hold.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Missing {
    /// `doi:…`. Only works with an identifier can appear here; see below.
    pub fingerprint: String,
    pub label: String,
    pub year: Option<i64>,
    /// The DOI as deposited, so the work can actually be fetched.
    pub doi: String,
    /// How many papers in the library cite it.
    pub cited_by: i64,
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

    /// Papers with an identifier whose reference list has never been fetched.
    ///
    /// Returned rather than counted so the caller can work through them and
    /// stop whenever it likes: fetching is a network request per paper to
    /// somebody else's service, and a job that cannot be stopped halfway is a
    /// job that should not be started.
    async fn unfetched(&self, library_id: i64, limit: u32) -> Result<Vec<(Key, String)>>;

    /// The works this library cites most and does not hold.
    ///
    /// This is the question a citation graph exists to answer. A paper cited by
    /// several things on the shelf and owned by none is, almost by definition,
    /// the next thing to read — and it is invisible in every other view,
    /// because nothing in the library is it.
    async fn missing(&self, library_id: i64, limit: u32) -> Result<Vec<Missing>>;
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
                         (source_id, kind, position, target_key, target_label,
                          target_year, target_doi)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    )
                    .map_err(sql_err)?;
                for (position, c) in citations.iter().enumerate() {
                    stmt.execute(params![
                        id,
                        CITES,
                        position as i64,
                        c.fingerprint,
                        c.label,
                        c.year,
                        c.doi
                    ])
                    .map_err(sql_err)?;
                    stored += 1;
                }
            }
            // Record that we asked, separately from what came back. A paper
            // with no deposited references leaves no rows at all, and without
            // this the next bulk run cannot tell that from never having asked.
            tx.execute(
                "INSERT INTO citation_fetches (item_id, kind, fetched_at, found)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(item_id, kind) DO UPDATE SET
                    fetched_at = excluded.fetched_at, found = excluded.found",
                params![id, CITES, yk_core::now_ms(), stored as i64],
            )
            .map_err(sql_err)?;

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
                    "SELECT r.position, r.target_key, r.target_label, r.target_year, i.key,
                            r.target_doi
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
                    "SELECT r.position, r.target_key, i.fields, i.year, i.key, r.target_doi
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
                        doi: r.get(5)?,
                    })
                })
                .map_err(sql_err)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn unfetched(&self, library_id: i64, limit: u32) -> Result<Vec<(Key, String)>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            // `fingerprint` is indexed and a DOI sorts under a common prefix,
            // so this is a range scan rather than a walk of the library.
            let mut stmt = conn
                .prepare_cached(
                    "SELECT i.key, i.fields FROM items i
                     WHERE i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
                       AND i.fingerprint LIKE 'doi:%'
                       AND NOT EXISTS (SELECT 1 FROM citation_fetches f
                                       WHERE f.item_id = i.id AND f.kind = ?2)
                     LIMIT ?3",
                )
                .map_err(sql_err)?;

            let rows = stmt
                .query_map(params![library_id, CITES, limit], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(sql_err)?;

            let mut out = Vec::new();
            for row in rows {
                let (key, fields) = row.map_err(sql_err)?;
                let (Ok(key), Some(doi)) = (Key::parse(&key), doi_of(&fields)) else { continue };
                out.push((key, doi));
            }
            Ok(out)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }

    async fn missing(&self, library_id: i64, limit: u32) -> Result<Vec<Missing>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;

            // Grouped by the cited work, counted by *distinct citing papers*
            // rather than by rows: a bibliography that lists the same work
            // twice says nothing about how central it is.
            //
            // Entries with no identifier are excluded, and deliberately. Prose
            // references cannot be grouped — the same paper appears in ten
            // bibliographies in ten different house styles — so counting them
            // would produce a ranking of formatting conventions.
            let mut stmt = conn
                .prepare_cached(
                    "SELECT r.target_key,
                            max(r.target_label) AS label,
                            max(r.target_year)  AS year,
                            max(r.target_doi)   AS doi,
                            count(DISTINCT r.source_id) AS citations
                     FROM item_relations r
                     CROSS JOIN items i ON i.id = r.source_id
                     WHERE r.kind = ?1 AND r.target_key != ''
                       AND i.library_id = ?2 AND i.deleted = 0
                       AND NOT EXISTS (SELECT 1 FROM items owned
                                       WHERE owned.library_id = ?2 AND owned.deleted = 0
                                         AND owned.fingerprint = r.target_key)
                     GROUP BY r.target_key
                     ORDER BY citations DESC, year DESC
                     LIMIT ?3",
                )
                .map_err(sql_err)?;

            let rows = stmt
                .query_map(params![CITES, library_id, limit], |r| {
                    Ok(Missing {
                        fingerprint: r.get(0)?,
                        label: r.get(1)?,
                        year: r.get(2)?,
                        doi: r.get(3)?,
                        cited_by: r.get(4)?,
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
        doi: r.get(5)?,
    })
}

/// The DOI as the item stores it, not as the fingerprint flattened it.
fn doi_of(fields: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(fields)
        .ok()?
        .get("DOI")?
        .as_str()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
}

fn title_of(fields: &str) -> String {
    serde_json::from_str::<serde_json::Value>(fields)
        .ok()
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
