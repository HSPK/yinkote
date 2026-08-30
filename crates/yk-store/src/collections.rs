//! Collections, tags and libraries.

use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use yk_core::model::*;
use yk_core::ports::*;
use yk_core::query::ItemFilter;
use crate::counts::Answer;
use yk_core::{Error, Key, Result};

use crate::db::{sql_err, write_tx, Db};
use crate::filter::Predicate;

fn bump(tx: &Connection, library_id: i64) -> Result<i64> {
    tx.execute("UPDATE libraries SET version = version + 1 WHERE id = ?1", params![library_id])
        .map_err(sql_err)?;
    tx.query_row("SELECT version FROM libraries WHERE id=?1", params![library_id], |r| r.get(0))
        .map_err(sql_err)
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteLibraryRepository {
    db: Db,
}

impl SqliteLibraryRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Create the personal library on first run. Returns its id.
    pub fn ensure_default(db: &Db, name: &str) -> Result<i64> {
        let conn = db.conn()?;
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM libraries ORDER BY id LIMIT 1", [], |r| r.get(0))
            .optional()
            .map_err(sql_err)?;
        if let Some(id) = existing {
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO libraries(type, name, version, created_at) VALUES ('user', ?1, 0, ?2)",
            params![name, yk_core::now_ms()],
        )
        .map_err(sql_err)?;
        Ok(conn.last_insert_rowid())
    }
}

fn map_library(r: &rusqlite::Row<'_>) -> rusqlite::Result<Library> {
    Ok(Library {
        id: r.get(0)?,
        r#type: r.get(1)?,
        name: r.get(2)?,
        version: r.get(3)?,
    })
}

#[async_trait]
impl LibraryRepository for SqliteLibraryRepository {
    async fn list(&self) -> Result<Vec<Library>> {
        self.db
            .call(|c| {
                let mut stmt = c
                    .prepare_cached("SELECT id, type, name, version FROM libraries ORDER BY id")
                    .map_err(sql_err)?;
                let out = stmt
                    .query_map([], map_library)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_err);
                out
            })
            .await
    }

    async fn get(&self, id: i64) -> Result<Library> {
        self.db
            .call(move |c| {
                c.query_row(
                    "SELECT id, type, name, version FROM libraries WHERE id=?1",
                    params![id],
                    map_library,
                )
                .optional()
                .map_err(sql_err)?
                .ok_or_else(|| Error::not_found(format!("library {id}")))
            })
            .await
    }

    async fn version(&self, id: i64) -> Result<i64> {
        self.db
            .call(move |c| {
                c.query_row("SELECT version FROM libraries WHERE id=?1", params![id], |r| r.get(0))
                    .optional()
                    .map_err(sql_err)?
                    .ok_or_else(|| Error::not_found(format!("library {id}")))
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteCollectionRepository {
    db: Db,
    /// The sidebar's list, remembered against the library version.
    ///
    /// Each row carries how many live items the collection holds, and that is
    /// the whole cost: 60,000 memberships each looked up in `items` to see
    /// whether they are in the trash. 27ms, on every sidebar load, and neither
    /// a correlated subquery nor one grouped join is cheaper than the other —
    /// the work is inherent, so the only way out is not repeating it.
    listing: std::sync::Arc<crate::counts::Versioned<Vec<Collection>>>,
}

impl SqliteCollectionRepository {
    pub fn new(db: Db) -> Self {
        Self { db, listing: Default::default() }
    }
}

impl SqliteCollectionRepository {
    async fn library_version(&self, library_id: i64) -> i64 {
        self.db
            .call(move |c| Ok(crate::counts::version_of(c, library_id)))
            .await
            .unwrap_or(-1)
    }

    /// Fresh membership, remembered counts.
    ///
    /// Everything in `C_SELECT` except the correlated count, which is the part
    /// worth deferring. A collection the cache has never seen reports zero
    /// until the recompute behind this request lands — honest for the case
    /// that produces one, which is a collection that has just been created.
    async fn reconcile(&self, library_id: i64, cached: Vec<Collection>) -> Result<Vec<Collection>> {
        let counts: std::collections::HashMap<String, i64> =
            cached.into_iter().map(|c| (c.key.to_string(), c.item_count)).collect();
        self.db
            .call(move |c| {
                let mut stmt = c.prepare_cached(SKELETON).map_err(sql_err)?;
                let rows: Vec<Collection> = stmt
                    .query_map(params![library_id], |r| {
                        let mut collection = map_skeleton(r)?;
                        collection.item_count =
                            counts.get(collection.key.as_str()).copied().unwrap_or(0);
                        Ok(collection)
                    })
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                Ok(rows)
            })
            .await
    }

    /// Read the list and remember it, with what it cost.
    async fn recompute(
        &self,
        library_id: i64,
        key: String,
        version: i64,
    ) -> Result<Vec<Collection>> {
        let listing = self.listing.clone();
        self.db
            .call(move |c| {
                let started = std::time::Instant::now();
                let sql = format!("{C_SELECT} WHERE c.library_id=?1 ORDER BY c.sort_index, c.name");
                let rows: Vec<Collection> = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id], map_collection)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                if version >= 0 {
                    listing.put_timed(key, version, rows.clone(), started.elapsed());
                }
                Ok(rows)
            })
            .await
    }
}

/// The count counts what clicking the row will show.
///
/// Browsing a collection includes its sub-collections -- `recursive` defaults
/// to true and there is a test saying so -- while this counted only direct
/// members. A shelf holding sixty papers with a sub-shelf of sixty more was
/// labelled 60 in the sidebar and listed 120 when opened. §3.223 fixed the
/// same disagreement between the sidebar and the footer for the library as a
/// whole; nesting is where it survived.
///
/// `DISTINCT`, because a paper filed in both a collection and its child is one
/// paper, and the list shows it once.
const C_SELECT: &str = "SELECT c.id, c.key, c.library_id, c.name, p.key, c.sort_index, \
     c.color, c.icon, c.version, c.date_added, c.date_modified, \
     (SELECT count(DISTINCT ci.item_id) \
        FROM collection_items ci JOIN items i ON i.id = ci.item_id \
       WHERE i.deleted = 0 AND ci.collection_id IN ( \
             WITH RECURSIVE sub(id) AS ( \
                 SELECT c.id UNION ALL \
                 SELECT d.id FROM collections d JOIN sub ON d.parent_id = sub.id) \
             SELECT id FROM sub)) \
     FROM collections c LEFT JOIN collections p ON p.id = c.parent_id";

/// The same rows as `C_SELECT` without the count, which is the expensive half.
const SKELETON: &str = "SELECT c.id, c.key, c.library_id, c.name, p.key, c.sort_index, \
     c.color, c.icon, c.version, c.date_added, c.date_modified \
     FROM collections c LEFT JOIN collections p ON p.id = c.parent_id \
     WHERE c.library_id=?1 ORDER BY c.sort_index, c.name";

/// Everything but `item_count`, which the caller fills in.
fn map_skeleton(r: &rusqlite::Row<'_>) -> rusqlite::Result<Collection> {
    let parent: Option<String> = r.get(4)?;
    Ok(Collection {
        key: Key::parse(&r.get::<_, String>(1)?).unwrap_or_else(|_| Key::generate()),
        library_id: r.get(2)?,
        name: r.get(3)?,
        parent_key: parent.and_then(|p| Key::parse(&p).ok()),
        sort_index: r.get(5)?,
        color: r.get(6)?,
        icon: r.get(7)?,
        version: r.get(8)?,
        date_added: r.get(9)?,
        date_modified: r.get(10)?,
        item_count: 0,
    })
}

fn map_collection(r: &rusqlite::Row<'_>) -> rusqlite::Result<Collection> {
    let parent: Option<String> = r.get(4)?;
    Ok(Collection {
        key: Key::parse(&r.get::<_, String>(1)?).unwrap_or_else(|_| Key::generate()),
        library_id: r.get(2)?,
        name: r.get(3)?,
        parent_key: parent.and_then(|p| Key::parse(&p).ok()),
        sort_index: r.get(5)?,
        color: r.get(6)?,
        icon: r.get(7)?,
        version: r.get(8)?,
        date_added: r.get(9)?,
        date_modified: r.get(10)?,
        item_count: r.get(11)?,
    })
}

/// Guard against making a collection its own ancestor.
fn would_cycle(tx: &Connection, id: i64, new_parent: i64) -> Result<bool> {
    let mut cur = Some(new_parent);
    let mut guard = 0;
    while let Some(c) = cur {
        if c == id {
            return Ok(true);
        }
        guard += 1;
        if guard > 1000 {
            return Ok(true);
        }
        cur = tx
            .query_row("SELECT parent_id FROM collections WHERE id=?1", params![c], |r| r.get(0))
            .optional()
            .map_err(sql_err)?
            .flatten();
    }
    Ok(false)
}

#[async_trait]
impl CollectionRepository for SqliteCollectionRepository {
    async fn list(&self, library_id: i64) -> Result<Vec<Collection>> {
        let key = format!("collections:{library_id}");
        let version = self.library_version(library_id).await;

        match self.listing.look_up(&key, version) {
            Answer::Fresh(rows) => return Ok(rows),
            // Handed back as it stands while a fresh one is computed behind
            // this request. Every edit anywhere in the library retires this
            // list — each row carries a live-item count — so waiting for it
            // would put 30ms on the first navigation after every change, for a
            // number that is a label beside a name.
            Answer::Stale(rows) => {
                if self.listing.claim(&key) {
                    let this = self.clone();
                    let key = key.clone();
                    tokio::spawn(async move {
                        let _ = this.recompute(library_id, key.clone(), version).await;
                        this.listing.release(&key);
                    });
                }
                // The counts may lag; *which collections exist* may not. A
                // stale listing was handed back whole, so a collection created
                // a moment ago was absent from the very next request and one
                // just deleted was still there — the sidebar disagreeing with
                // the library about what it contains, until some later edit
                // happened to refresh it.
                //
                // Only the per-collection live-item count is expensive (60,000
                // memberships, 27ms); the rows themselves are a plain read. So
                // the rows are re-read and the cached counts are carried over
                // by key, which keeps the saving and drops the lie.
                return self.reconcile(library_id, rows).await;
            }
            Answer::Missing => {}
        }
        self.recompute(library_id, key, version).await
    }

    async fn count(&self, library_id: i64) -> Result<i64> {
        self.db
            .call(move |c| {
                c.query_row(
                    "SELECT count(*) FROM collections WHERE library_id = ?1",
                    params![library_id],
                    |r| r.get(0),
                )
                .map_err(sql_err)
            })
            .await
    }

    async fn get(&self, library_id: i64, key: &Key) -> Result<Collection> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let sql = format!("{C_SELECT} WHERE c.library_id=?1 AND c.key=?2");
                c.prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_row(params![library_id, key.as_str()], map_collection)
                    .optional()
                    .map_err(sql_err)?
                    .ok_or_else(|| Error::not_found(format!("collection {key}")))
            })
            .await
    }

    async fn create(&self, library_id: i64, draft: CollectionDraft) -> Result<Collection> {
        self.db
            .call(move |c| {
                let name = draft.name.trim().to_string();
                if name.is_empty() {
                    return Err(Error::invalid("collection name must not be empty"));
                }
                let tx = write_tx(c)?;
                let parent_id: Option<i64> = match &draft.parent_key {
                    Some(k) => Some(
                        tx.query_row(
                            "SELECT id FROM collections WHERE library_id=?1 AND key=?2",
                            params![library_id, k.as_str()],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(sql_err)?
                        .ok_or_else(|| Error::not_found(format!("collection {k}")))?,
                    ),
                    None => None,
                };
                let version = bump(&tx, library_id)?;
                let now = yk_core::now_ms();
                let key = draft.key.clone().unwrap_or_else(Key::generate);
                let sort_index = draft.sort_index.unwrap_or_else(|| {
                    tx.query_row(
                        "SELECT COALESCE(MAX(sort_index), 0) + 1 FROM collections \
                         WHERE library_id=?1 AND parent_id IS ?2",
                        params![library_id, parent_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(0.0)
                });
                tx.execute(
                    "INSERT INTO collections
                       (library_id, key, parent_id, name, sort_index, color, icon, version,
                        date_added, date_modified)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                    params![
                        library_id,
                        key.as_str(),
                        parent_id,
                        name,
                        sort_index,
                        draft.color.as_deref(),
                        draft.icon.as_deref(),
                        version,
                        now
                    ],
                )
                .map_err(sql_err)?;
                tx.commit().map_err(sql_err)?;
                Ok(Collection {
                    key,
                    library_id,
                    name,
                    parent_key: draft.parent_key,
                    sort_index,
                    color: draft.color,
                    icon: draft.icon,
                    version,
                    date_added: now,
                    date_modified: now,
                    item_count: 0,
                })
            })
            .await
    }

    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: CollectionPatch,
    ) -> Result<Collection> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let id: i64 = tx
                    .query_row(
                        "SELECT id FROM collections WHERE library_id=?1 AND key=?2",
                        params![library_id, key.as_str()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql_err)?
                    .ok_or_else(|| Error::not_found(format!("collection {key}")))?;

                if let Some(name) = &patch.name {
                    let name = name.trim();
                    if name.is_empty() {
                        return Err(Error::invalid("collection name must not be empty"));
                    }
                    tx.execute(
                        "UPDATE collections SET name=?1 WHERE id=?2",
                        params![name, id],
                    )
                    .map_err(sql_err)?;
                }
                if let Some(parent) = &patch.parent_key {
                    let parent_id: Option<i64> = match parent {
                        Some(k) => {
                            let pid: i64 = tx
                                .query_row(
                                    "SELECT id FROM collections WHERE library_id=?1 AND key=?2",
                                    params![library_id, k.as_str()],
                                    |r| r.get(0),
                                )
                                .optional()
                                .map_err(sql_err)?
                                .ok_or_else(|| Error::not_found(format!("collection {k}")))?;
                            if would_cycle(&tx, id, pid)? {
                                return Err(Error::invalid("cannot nest a collection inside itself"));
                            }
                            Some(pid)
                        }
                        None => None,
                    };
                    tx.execute(
                        "UPDATE collections SET parent_id=?1 WHERE id=?2",
                        params![parent_id, id],
                    )
                    .map_err(sql_err)?;
                }
                if let Some(si) = patch.sort_index {
                    tx.execute("UPDATE collections SET sort_index=?1 WHERE id=?2", params![si, id])
                        .map_err(sql_err)?;
                }
                if let Some(color) = &patch.color {
                    tx.execute(
                        "UPDATE collections SET color=?1 WHERE id=?2",
                        params![color.as_deref(), id],
                    )
                    .map_err(sql_err)?;
                }
                if let Some(icon) = &patch.icon {
                    tx.execute(
                        "UPDATE collections SET icon=?1 WHERE id=?2",
                        params![icon.as_deref(), id],
                    )
                    .map_err(sql_err)?;
                }

                let version = bump(&tx, library_id)?;
                tx.execute(
                    "UPDATE collections SET version=?1, date_modified=?2 WHERE id=?3",
                    params![version, yk_core::now_ms(), id],
                )
                .map_err(sql_err)?;

                let sql = format!("{C_SELECT} WHERE c.id=?1");
                let out = tx
                    .query_row(&sql, params![id], map_collection)
                    .map_err(sql_err)?;
                tx.commit().map_err(sql_err)?;
                Ok(out)
            })
            .await
    }

    async fn delete(&self, library_id: i64, key: &Key, recursive: bool) -> Result<u64> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let id: i64 = tx
                    .query_row(
                        "SELECT id FROM collections WHERE library_id=?1 AND key=?2",
                        params![library_id, key.as_str()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql_err)?
                    .ok_or_else(|| Error::not_found(format!("collection {key}")))?;

                if !recursive {
                    // Re-parent children instead of cascading them away.
                    let parent: Option<i64> = tx
                        .query_row("SELECT parent_id FROM collections WHERE id=?1", params![id], |r| {
                            r.get(0)
                        })
                        .map_err(sql_err)?;
                    tx.execute(
                        "UPDATE collections SET parent_id=?1 WHERE parent_id=?2",
                        params![parent, id],
                    )
                    .map_err(sql_err)?;
                }
                let n = tx
                    .execute("DELETE FROM collections WHERE id=?1", params![id])
                    .map_err(sql_err)? as u64;
                bump(&tx, library_id)?;
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }

    async fn descendants(&self, library_id: i64, key: &Key) -> Result<Vec<Key>> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let mut stmt = c
                    .prepare_cached(
                        "WITH RECURSIVE sub(id) AS (
                            SELECT id FROM collections WHERE library_id=?1 AND key=?2
                            UNION ALL
                            SELECT c.id FROM collections c JOIN sub ON c.parent_id = sub.id
                         ) SELECT key FROM collections WHERE id IN (SELECT id FROM sub)",
                    )
                    .map_err(sql_err)?;
                let keys: Vec<String> = stmt
                    .query_map(params![library_id, key.as_str()], |r| r.get(0))
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                Ok(keys.iter().filter_map(|k| Key::parse(k).ok()).collect())
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteTagRepository {
    db: Db,
}

impl SqliteTagRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn map_tag(r: &rusqlite::Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        name: r.get(0)?,
        color: r.get(1)?,
        count: r.get(2)?,
        r#type: r.get::<_, i64>(3)? as u8,
    })
}

#[async_trait]
impl TagRepository for SqliteTagRepository {
    async fn list(&self, library_id: i64, prefix: Option<&str>, limit: u32) -> Result<Vec<Tag>> {
        let like = prefix.map(|p| format!("%{p}%"));
        self.db
            .call(move |c| {
                let mut sql = String::from(
                    "SELECT t.name, t.color, count(it.item_id), MIN(COALESCE(it.type,0)) \
                     FROM tags t \
                     LEFT JOIN item_tags it ON it.tag_id = t.id \
                     LEFT JOIN items i ON i.id = it.item_id AND i.deleted = 0 \
                     WHERE t.library_id = ?1 AND (it.item_id IS NULL OR i.id IS NOT NULL)",
                );
                let mut args: Vec<rusqlite::types::Value> =
                    vec![rusqlite::types::Value::Integer(library_id)];
                if let Some(l) = &like {
                    sql.push_str(" AND t.name LIKE ?2");
                    args.push(rusqlite::types::Value::Text(l.clone()));
                }
                sql.push_str(" GROUP BY t.id ORDER BY count(it.item_id) DESC, t.name LIMIT ");
                sql.push_str(&limit.to_string());

                c.prepare(&sql)
                    .map_err(sql_err)?
                    .query_map(params_from_iter(args), map_tag)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)
            })
            .await
    }

    async fn count(&self, library_id: i64) -> Result<i64> {
        self.db
            .call(move |c| {
                c.query_row(
                    "SELECT count(*) FROM tags WHERE library_id = ?1",
                    params![library_id],
                    |r| r.get(0),
                )
                .map_err(sql_err)
            })
            .await
    }

    async fn rename(&self, library_id: i64, from: &str, to: &str) -> Result<u64> {
        let (from, to) = (from.to_string(), to.trim().to_string());
        if to.is_empty() {
            return Err(Error::invalid("tag name must not be empty"));
        }
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let src: Option<i64> = tx
                    .query_row(
                        "SELECT id FROM tags WHERE library_id=?1 AND name=?2",
                        params![library_id, from],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql_err)?;
                let Some(src) = src else { return Ok(0) };

                let dst: Option<i64> = tx
                    .query_row(
                        "SELECT id FROM tags WHERE library_id=?1 AND name=?2",
                        params![library_id, to],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql_err)?;

                let n = match dst {
                    // Merge into the existing tag.
                    Some(dst) if dst != src => {
                        let n = tx
                            .execute(
                                "UPDATE OR IGNORE item_tags SET tag_id=?1 WHERE tag_id=?2",
                                params![dst, src],
                            )
                            .map_err(sql_err)? as u64;
                        tx.execute("DELETE FROM tags WHERE id=?1", params![src]).map_err(sql_err)?;
                        n
                    }
                    _ => {
                        tx.execute("UPDATE tags SET name=?1 WHERE id=?2", params![to, src])
                            .map_err(sql_err)?;
                        tx.query_row(
                            "SELECT count(*) FROM item_tags WHERE tag_id=?1",
                            params![src],
                            |r| r.get::<_, i64>(0),
                        )
                        .map_err(sql_err)? as u64
                    }
                };
                bump(&tx, library_id)?;
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }

    async fn delete(&self, library_id: i64, name: &str) -> Result<u64> {
        let name = name.to_string();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let n = tx
                    .execute(
                        "DELETE FROM tags WHERE library_id=?1 AND name=?2",
                        params![library_id, name],
                    )
                    .map_err(sql_err)? as u64;
                bump(&tx, library_id)?;
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }

    async fn set_color(&self, library_id: i64, name: &str, color: Option<&str>) -> Result<()> {
        let (name, color) = (name.to_string(), color.map(str::to_string));
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                tx.execute(
                    "INSERT INTO tags(library_id, name, color) VALUES (?1,?2,?3) \
                     ON CONFLICT(library_id, name) DO UPDATE SET color = excluded.color",
                    params![library_id, name, color],
                )
                .map_err(sql_err)?;
                // Like every other write: the version bump is what tells sync
                // clients and read caches that something changed.
                bump(&tx, library_id)?;
                tx.commit().map_err(sql_err)?;
                Ok(())
            })
            .await
    }

    async fn facets(&self, filter: &ItemFilter, limit: u32) -> Result<Vec<Tag>> {
        let filter = filter.clone();
        self.db
            .call(move |c| {
                let p = Predicate::build(&filter, None);
                let sql = format!(
                    "SELECT t.name, t.color, count(*), MIN(it.type) FROM item_tags it \
                     JOIN tags t ON t.id = it.tag_id \
                     JOIN items i ON i.id = it.item_id \
                     WHERE {} GROUP BY t.id ORDER BY count(*) DESC, t.name LIMIT {limit}",
                    p.sql
                );
                c.prepare(&sql)
                    .map_err(sql_err)?
                    .query_map(params_from_iter(p.params.iter()), map_tag)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use yk_core::ports::CollectionRepository;

    /// A stale listing may be out of date about counts. It may not be out of
    /// date about what exists.
    ///
    /// The listing is cached against the library version and, when recomputing
    /// it is slow, the previous one is handed back while a fresh one is built
    /// behind the request. That is right for the number beside a name and
    /// wrong for the names: a collection created a moment earlier was missing
    /// from the very next listing, and one just deleted was still in it.
    ///
    /// It surfaced as an intermittent smoke failure — "colour saved" reading
    /// empty — because whether the cache defers depends on whether the last
    /// recompute crossed 20ms, so it appeared only once the library was big
    /// enough. The test does not wait for that: it seeds the cache with an
    /// expensive, out-of-date entry, which is exactly the state being
    /// described.
    #[tokio::test]
    async fn a_deferred_listing_still_knows_what_exists() {
        // Through `Store` so the schema and the default library are set up
        // the one way they are everywhere else, with a repository of our own
        // beside it because the cache being described is a private field.
        let store = crate::Store::in_memory().unwrap();
        let lib = store.default_library;
        let repo = SqliteCollectionRepository::new(store.db().clone());

        let first = repo
            .create(lib, CollectionDraft { name: "Already here".into(), ..Default::default() })
            .await
            .unwrap();
        let listing = repo.list(lib).await.unwrap();
        assert_eq!(listing.len(), 1);

        // Say the last recompute was expensive, and that what we hold is from
        // an older version of the library. Both are true of a real library
        // between an edit and the refresh behind it.
        repo.listing.put_timed(
            format!("collections:{lib}"),
            0,
            listing,
            Duration::from_millis(500),
        );

        let second = repo
            .create(lib, CollectionDraft { name: "Just made".into(), ..Default::default() })
            .await
            .unwrap();

        let names: Vec<String> =
            repo.list(lib).await.unwrap().into_iter().map(|c| c.name).collect();
        assert!(
            names.iter().any(|n| n == "Just made"),
            "a collection that exists was missing from the listing: {names:?}"
        );
        assert!(names.iter().any(|n| n == "Already here"), "the older one was dropped");

        // And a deletion is honoured just as promptly.
        repo.delete(lib, &first.key, false).await.unwrap();
        repo.listing.put_timed(
            format!("collections:{lib}"),
            0,
            vec![first.clone(), second.clone()],
            Duration::from_millis(500),
        );
        let names: Vec<String> =
            repo.list(lib).await.unwrap().into_iter().map(|c| c.name).collect();
        assert!(!names.iter().any(|n| n == "Already here"), "a deleted collection was listed");
    }
}
