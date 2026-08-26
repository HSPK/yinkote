//! The relationship graph, computed rather than stored.
//!
//! The design document called for `graph_nodes` and `graph_edges` tables kept
//! up to date beside the items. That is the right shape once citation data
//! exists, because a citation is a fact from outside that has to live
//! somewhere. It is the wrong shape for the edges this library can actually
//! derive today — shared tags, shared authors, shared collections — because
//! every one of them is already implied by the items. Materialising them would
//! be a cache, and a cache of something derivable can disagree with the thing
//! it derives from. A graph that quietly disagrees with the library is worse
//! than no graph, because it looks authoritative.
//!
//! So the graph is a query. It is always a neighbourhood, never the whole
//! library: a hundred thousand nodes is not a picture, and nobody can read one.

use async_trait::async_trait;
use rusqlite::params;
use serde::Serialize;
use yk_core::{Error, Key, Result};

use crate::db::{sql_err, Db};

/// How a neighbour is related to the focus.
///
/// The kind is carried through to the client so an edge can be explained. An
/// unexplained edge in a graph is a claim the user has to take on trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    /// Tags in common.
    Tag,
    /// The same leading author.
    Author,
    /// Filed together.
    Collection,
}

/// One item related to the focus.
#[derive(Debug, Clone, Serialize)]
pub struct Neighbour {
    pub key: Key,
    pub title: String,
    pub year: Option<i64>,
    #[serde(rename = "itemType")]
    pub item_type: String,
    pub relation: Relation,
    /// How many tags, collections or authors are shared. Similarity edges
    /// carry a score instead, and are added by the search engine.
    pub weight: f64,
}

/// A tag on more than this share of the library says nothing about
/// relatedness.
///
/// `to-read` on four thousand items does not make those four thousand items
/// related; it makes them unread. Excluding such tags is not only a
/// performance guard — though it is that, and a large one, since one popular
/// tag can make the join scan most of `item_tags` — it is what stops the graph
/// filling with edges that mean nothing.
const COMMON_TAG_SHARE: f64 = 0.05;

/// Never fewer than this, so a small library still has a graph.
const COMMON_TAG_FLOOR: i64 = 50;

#[async_trait]
pub trait GraphRepository: Send + Sync {
    /// Items related to `key`, best first, at most `limit` of each relation.
    async fn neighbours(&self, library_id: i64, key: &Key, limit: u32)
        -> Result<Vec<Neighbour>>;
}

#[derive(Clone)]
pub struct SqliteGraphRepository {
    db: Db,
}

impl SqliteGraphRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GraphRepository for SqliteGraphRepository {
    async fn neighbours(
        &self,
        library_id: i64,
        key: &Key,
        limit: u32,
    ) -> Result<Vec<Neighbour>> {
        let db = self.db.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn()?;
            let id: i64 = conn
                .query_row(
                    "SELECT id FROM items WHERE library_id=?1 AND key=?2",
                    params![library_id, key.as_str()],
                    |r| r.get(0),
                )
                .map_err(|_| Error::not_found(format!("item {key}")))?;

            let ceiling = common_tag_ceiling(&conn, library_id)?;
            let mut out = Vec::new();
            out.extend(by_tag(&conn, library_id, id, ceiling, limit)?);
            out.extend(by_author(&conn, library_id, id, limit)?);
            out.extend(by_collection(&conn, library_id, id, limit)?);
            Ok(out)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }
}

/// The point above which a tag is too common to mean anything.
fn common_tag_ceiling(conn: &rusqlite::Connection, library_id: i64) -> Result<i64> {
    let items: i64 = conn
        .query_row(
            "SELECT count(*) FROM items WHERE library_id=?1 AND deleted=0 AND parent_id IS NULL",
            params![library_id],
            |r| r.get(0),
        )
        .map_err(sql_err)?;
    Ok((((items as f64) * COMMON_TAG_SHARE) as i64).max(COMMON_TAG_FLOOR))
}

/// Items sharing tags, most shared first.
fn by_tag(
    conn: &rusqlite::Connection,
    library_id: i64,
    id: i64,
    ceiling: i64,
    limit: u32,
) -> Result<Vec<Neighbour>> {
    // The inner select names the focus item's *uncommon* tags. Doing it here
    // rather than joining `item_tags` to itself keeps the outer scan to the
    // rows that carry one of a handful of tags.
    let sql = "SELECT i.key, i.fields, i.item_type, i.year, count(*) AS shared
               FROM item_tags it
               JOIN items i ON i.id = it.item_id
               WHERE it.tag_id IN (
                     SELECT t.id FROM item_tags mine
                     JOIN tags t ON t.id = mine.tag_id
                     WHERE mine.item_id = ?2
                       AND (SELECT count(*) FROM item_tags c WHERE c.tag_id = t.id) <= ?3)
                 AND it.item_id != ?2
                 AND i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
               GROUP BY i.id
               ORDER BY shared DESC, i.year DESC
               LIMIT ?4";
    query(conn, sql, params![library_id, id, ceiling, limit], Relation::Tag)
}

/// Items by the same leading author.
///
/// `sort_creator` is the normalised leading name and is indexed, so this is a
/// lookup rather than a scan. It is only the *leading* author: a full
/// co-authorship graph needs a creators table, and inventing one to answer a
/// question nobody has asked yet is how schemas rot.
fn by_author(
    conn: &rusqlite::Connection,
    library_id: i64,
    id: i64,
    limit: u32,
) -> Result<Vec<Neighbour>> {
    let sql = "SELECT i.key, i.fields, i.item_type, i.year, 1
               FROM items i
               WHERE i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
                 AND i.id != ?2
                 AND i.sort_creator != ''
                 AND i.sort_creator = (SELECT sort_creator FROM items WHERE id = ?2)
               ORDER BY i.year DESC
               LIMIT ?3";
    query(conn, sql, params![library_id, id, limit], Relation::Author)
}

/// Items filed in the same collections.
fn by_collection(
    conn: &rusqlite::Connection,
    library_id: i64,
    id: i64,
    limit: u32,
) -> Result<Vec<Neighbour>> {
    let sql = "SELECT i.key, i.fields, i.item_type, i.year, count(*) AS shared
               FROM collection_items ci
               JOIN items i ON i.id = ci.item_id
               WHERE ci.collection_id IN
                     (SELECT collection_id FROM collection_items WHERE item_id = ?2)
                 AND ci.item_id != ?2
                 AND i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
               GROUP BY i.id
               ORDER BY shared DESC, i.year DESC
               LIMIT ?3";
    query(conn, sql, params![library_id, id, limit], Relation::Collection)
}

fn query(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl rusqlite::Params,
    relation: Relation,
) -> Result<Vec<Neighbour>> {
    let mut stmt = conn.prepare_cached(sql).map_err(sql_err)?;
    let rows = stmt
        .query_map(params, |r| {
            let fields: String = r.get(1)?;
            Ok(Neighbour {
                key: Key::parse(&r.get::<_, String>(0)?).unwrap_or_else(|_| Key::generate()),
                title: title_of(&fields),
                item_type: r.get(2)?,
                year: r.get(3)?,
                weight: r.get::<_, i64>(4)? as f64,
                relation,
            })
        })
        .map_err(sql_err)?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
}

/// A node needs a label, and only the title will do.
fn title_of(fields: &str) -> String {
    serde_json::from_str::<serde_json::Value>(fields)
        .ok()
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
