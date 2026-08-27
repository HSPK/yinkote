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
    /// Cites some of the same works.
    ///
    /// Bibliographic coupling: two papers that lean on the same references
    /// are working on the same problem, whether or not anyone has tagged them
    /// that way. It is the one edge here the library does not already imply —
    /// it comes from the reference lists, which are facts from the world.
    Coupling,
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
            out.extend(by_coupling(&conn, library_id, id, limit)?);
            Ok(out)
        })
        .await
        .map_err(|e| Error::internal(e.to_string()))?
    }
}

/// The point above which a tag is too common to mean anything.
///
/// Deliberately counts *every* row rather than only top-level items. Adding
/// `parent_id IS NULL` reads the table instead of the index — 156 ms against
/// 3.7 ms on a hundred thousand items — and all it buys is excluding
/// attachments and notes from a number that is then multiplied by a twentieth.
/// Precision nobody can perceive is not worth forty times the cost.
fn common_tag_ceiling(conn: &rusqlite::Connection, library_id: i64) -> Result<i64> {
    let items: i64 = conn
        .query_row(
            "SELECT count(*) FROM items WHERE library_id=?1 AND deleted=0",
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
    // The inner select names the focus item's *uncommon* tags, so the outer
    // scan only touches rows carrying one of a handful of tags.
    //
    // `CROSS JOIN` pins the join order. Left to itself the planner drove the
    // query from `items` using `parent_id IS NULL` — a predicate matching the
    // entire library — and probed the tags for each of a hundred thousand rows:
    // 61 ms against 8. The keyword changes no results, only which table is the
    // outer loop, and here only one choice is sane.
    query(conn, TAG_SQL, params![library_id, id, ceiling, limit], Relation::Tag)
}

pub(crate) const TAG_SQL: &str = "SELECT i.key, i.fields, i.item_type, i.year, count(*) AS shared
               FROM item_tags it
               CROSS JOIN items i ON i.id = it.item_id
               WHERE it.tag_id IN (
                     SELECT t.id FROM item_tags mine
                     JOIN tags t ON t.id = mine.tag_id
                     WHERE mine.item_id = ?2
                       AND (SELECT count(*) FROM
                            (SELECT 1 FROM item_tags c WHERE c.tag_id = t.id LIMIT ?3 + 1))
                           <= ?3)
                 AND it.item_id != ?2
                 AND i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
               GROUP BY i.id
               ORDER BY shared DESC, i.year DESC
               LIMIT ?4";

/// How many of a prolific author's works are considered before picking the
/// newest few.
///
/// The inner lookup is what keeps this bounded, and the bound is what makes it
/// safe: without it, `ORDER BY year` tempts the planner to walk the year index
/// looking for a name, which costs 150 ms for an author with *no* other works —
/// the most common case in a real library, since most authors appear once.
/// Seeking by name first and sorting the handful found takes 0.1 ms.
const AUTHOR_CANDIDATES: u32 = 200;

/// Items by the same leading author.
///
/// Only the *leading* author: a full co-authorship graph needs a creators
/// table, and inventing one to answer a question nobody has asked yet is how
/// schemas rot.
///
/// An item with no creator matches nothing, and is asked about not at all —
/// empty is not a name, and joining every anonymous item to every other is the
/// loudest possible wrong answer.
fn by_author(
    conn: &rusqlite::Connection,
    library_id: i64,
    id: i64,
    limit: u32,
) -> Result<Vec<Neighbour>> {
    let creator: String = conn
        .query_row("SELECT sort_creator FROM items WHERE id=?1", params![id], |r| r.get(0))
        .map_err(sql_err)?;
    if creator.is_empty() {
        return Ok(Vec::new());
    }

    // The inner select seeks by name — an equality with no ordering, which is
    // exactly what `idx_items_creator` answers. Sorting happens afterwards, on
    // the few hundred rows it returned rather than on the whole library.
    query(
        conn,
        AUTHOR_SQL,
        params![library_id, id, creator, limit, AUTHOR_CANDIDATES],
        Relation::Author,
    )
}

pub(crate) const AUTHOR_SQL: &str = "SELECT i.key, i.fields, i.item_type, i.year, 1
               FROM items i
               WHERE i.id IN (SELECT id FROM items
                              WHERE library_id = ?1 AND deleted = 0
                                AND sort_creator = ?3 AND id != ?2
                              LIMIT ?5)
                 AND i.parent_id IS NULL
               ORDER BY i.year DESC
               LIMIT ?4";

/// Items filed in the same collections.
///
/// `CROSS JOIN` for the same reason as the tag query, and it mattered more: the
/// planner walked the whole library to answer a question about an item in no
/// collections at all — 28 ms to return nothing, against a hundredth of one.
fn by_collection(
    conn: &rusqlite::Connection,
    library_id: i64,
    id: i64,
    limit: u32,
) -> Result<Vec<Neighbour>> {
    query(conn, COLLECTION_SQL, params![library_id, id, limit], Relation::Collection)
}

pub(crate) const COLLECTION_SQL: &str = "SELECT i.key, i.fields, i.item_type, i.year, count(*) AS shared
               FROM collection_items ci
               CROSS JOIN items i ON i.id = ci.item_id
               WHERE ci.collection_id IN
                     (SELECT collection_id FROM collection_items WHERE item_id = ?2)
                 AND ci.item_id != ?2
                 AND i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
               GROUP BY i.id
               ORDER BY shared DESC, i.year DESC
               LIMIT ?3";

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

/// A reference shared by more than this many papers says nothing.
///
/// Every paper in a field cites its field's founding text; an edge drawn from
/// that is an edge between everything and everything. The same reasoning as
/// `COMMON_TAG_SHARE`, and the same shape of guard.
const COMMON_REFERENCE_CEILING: i64 = 50;

/// At least this many shared references before an edge is worth drawing.
///
/// One reference in common is a coincidence — two papers in a field will share
/// a review article without being about the same thing. Two is a pattern.
const MIN_SHARED_REFERENCES: i64 = 2;

/// Papers that cite some of the same works.
///
/// Only references the publisher gave an identifier to: an entry with no DOI
/// is matched by nothing but its label, and two bibliographies rarely spell
/// the same paper the same way. Coupling on a mistyped label is a wrong edge
/// stated with confidence, which is worse than a missing one.
fn by_coupling(
    conn: &rusqlite::Connection,
    library_id: i64,
    id: i64,
    limit: u32,
) -> Result<Vec<Neighbour>> {
    query(
        conn,
        COUPLING_SQL,
        params![library_id, id, limit, COMMON_REFERENCE_CEILING, MIN_SHARED_REFERENCES],
        Relation::Coupling,
    )
}

pub(crate) const COUPLING_SQL: &str =
    "SELECT i.key, i.fields, i.item_type, i.year, count(*) AS shared
               FROM item_relations theirs
               CROSS JOIN items i ON i.id = theirs.source_id
               WHERE theirs.target_key IN (
                     SELECT mine.target_key FROM item_relations mine
                     WHERE mine.source_id = ?2 AND mine.target_key != ''
                       AND (SELECT count(*) FROM
                            (SELECT 1 FROM item_relations c
                             WHERE c.target_key = mine.target_key LIMIT ?4 + 1))
                           <= ?4)
                 AND theirs.source_id != ?2
                 AND i.library_id = ?1 AND i.deleted = 0 AND i.parent_id IS NULL
               GROUP BY i.id
               HAVING shared >= ?5
               ORDER BY shared DESC, i.year DESC
               LIMIT ?3";

#[cfg(test)]
mod tests;
