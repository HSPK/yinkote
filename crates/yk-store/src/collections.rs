//! Collections, tags and libraries.

use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use yk_core::model::*;
use yk_core::ports::*;
use yk_core::query::ItemFilter;
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
}

impl SqliteCollectionRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

const C_SELECT: &str = "SELECT c.id, c.key, c.library_id, c.name, p.key, c.sort_index, \
     c.color, c.icon, c.version, \
     (SELECT count(*) FROM collection_items ci JOIN items i ON i.id = ci.item_id \
      WHERE ci.collection_id = c.id AND i.deleted = 0) \
     FROM collections c LEFT JOIN collections p ON p.id = c.parent_id";

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
        item_count: r.get(9)?,
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
        self.db
            .call(move |c| {
                let sql = format!("{C_SELECT} WHERE c.library_id=?1 ORDER BY c.sort_index, c.name");
                c.prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id], map_collection)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
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
                       (library_id, key, parent_id, name, sort_index, color, icon, version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        library_id,
                        key.as_str(),
                        parent_id,
                        name,
                        sort_index,
                        draft.color.as_deref(),
                        draft.icon.as_deref(),
                        version
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
                tx.execute("UPDATE collections SET version=?1 WHERE id=?2", params![version, id])
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
