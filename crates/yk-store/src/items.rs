//! SQLite implementation of the item, collection, tag and settings ports.

use std::collections::HashMap;

use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde_json::Value;
use yk_core::model::*;
use yk_core::ports::*;
use yk_core::query::*;
use yk_core::schema::schema;
use yk_core::{text, Error, Key, Result};

use crate::db::{sql_err, write_tx, Db};
use crate::filter::{order_by, placeholders, Predicate};
use crate::index;

const COLS: &str = "i.id, i.key, i.library_id, i.item_type, p.key, i.fields, i.creators, \
                    i.version, i.deleted, i.date_added, i.date_modified";
const FROM: &str = "FROM items i LEFT JOIN items p ON p.id = i.parent_id";

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

fn map_row(row: &Row<'_>) -> rusqlite::Result<(i64, Item)> {
    let id: i64 = row.get(0)?;
    let fields_raw: String = row.get(5)?;
    let creators_raw: String = row.get(6)?;
    let parent: Option<String> = row.get(4)?;

    let item = Item {
        key: Key::parse(&row.get::<_, String>(1)?).unwrap_or_else(|_| Key::generate()),
        library_id: row.get(2)?,
        item_type: row.get(3)?,
        parent_key: parent.and_then(|p| Key::parse(&p).ok()),
        fields: serde_json::from_str(&fields_raw).unwrap_or_default(),
        creators: serde_json::from_str(&creators_raw).unwrap_or_default(),
        tags: Vec::new(),
        collections: Vec::new(),
        version: row.get(7)?,
        deleted: row.get::<_, i64>(8)? != 0,
        date_added: row.get(9)?,
        date_modified: row.get(10)?,
    };
    Ok((id, item))
}

/// Fill in tags and collections for a page of items using two batched queries
/// instead of N+1.
fn hydrate(conn: &Connection, rows: &mut [(i64, Item)]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
    let mut by_id: HashMap<i64, usize> = HashMap::with_capacity(rows.len());
    for (idx, (id, _)) in rows.iter().enumerate() {
        by_id.insert(*id, idx);
    }

    // In runs. One placeholder per row means a ceiling, and a ceiling on a
    // shared helper is a failure waiting for whichever caller grows first.
    for run in crate::filter::chunks(&ids) {
        let ph = placeholders(run.len());

        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT it.item_id, t.name, it.type FROM item_tags it \
                 JOIN tags t ON t.id = it.tag_id WHERE it.item_id IN ({ph}) ORDER BY t.name"
            ))
            .map_err(sql_err)?;
        let mut cur = stmt.query(params_from_iter(run.iter())).map_err(sql_err)?;
        while let Some(r) = cur.next().map_err(sql_err)? {
            let id: i64 = r.get(0).map_err(sql_err)?;
            if let Some(&idx) = by_id.get(&id) {
                rows[idx].1.tags.push(ItemTag {
                    tag: r.get(1).map_err(sql_err)?,
                    r#type: r.get::<_, i64>(2).map_err(sql_err)? as u8,
                });
            }
        }
        drop(cur);
        drop(stmt);

        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT ci.item_id, c.key FROM collection_items ci \
                 JOIN collections c ON c.id = ci.collection_id WHERE ci.item_id IN ({ph})"
            ))
            .map_err(sql_err)?;
        let mut cur = stmt.query(params_from_iter(run.iter())).map_err(sql_err)?;
        while let Some(r) = cur.next().map_err(sql_err)? {
            let id: i64 = r.get(0).map_err(sql_err)?;
            if let Some(&idx) = by_id.get(&id) {
                if let Ok(k) = Key::parse(&r.get::<_, String>(1).map_err(sql_err)?) {
                    rows[idx].1.collections.push(k);
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Write helpers
// ---------------------------------------------------------------------------

/// Advance the library version counter and return the new value. Every write
/// goes through here, which is what makes delta sync possible.
fn bump_version(tx: &Connection, library_id: i64) -> Result<i64> {
    let n = tx
        .execute("UPDATE libraries SET version = version + 1 WHERE id = ?1", params![library_id])
        .map_err(sql_err)?;
    if n == 0 {
        return Err(Error::not_found(format!("library {library_id}")));
    }
    tx.query_row("SELECT version FROM libraries WHERE id = ?1", params![library_id], |r| r.get(0))
        .map_err(sql_err)
}

fn collection_id(tx: &Connection, library_id: i64, key: &Key) -> Result<i64> {
    tx.query_row(
        "SELECT id FROM collections WHERE library_id = ?1 AND key = ?2",
        params![library_id, key.as_str()],
        |r| r.get(0),
    )
    .optional()
    .map_err(sql_err)?
    .ok_or_else(|| Error::not_found(format!("collection {key}")))
}

fn item_id(tx: &Connection, library_id: i64, key: &Key) -> Result<i64> {
    tx.query_row(
        "SELECT id FROM items WHERE library_id = ?1 AND key = ?2",
        params![library_id, key.as_str()],
        |r| r.get(0),
    )
    .optional()
    .map_err(sql_err)?
    .ok_or_else(|| Error::not_found(format!("item {key}")))
}

fn tag_id(tx: &Connection, library_id: i64, name: &str) -> Result<i64> {
    tx.execute(
        "INSERT INTO tags(library_id, name) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
        params![library_id, name],
    )
    .map_err(sql_err)?;
    tx.query_row(
        "SELECT id FROM tags WHERE library_id = ?1 AND name = ?2",
        params![library_id, name],
        |r| r.get(0),
    )
    .map_err(sql_err)
}

fn set_tags(tx: &Connection, library_id: i64, id: i64, tags: &[ItemTag]) -> Result<()> {
    tx.execute("DELETE FROM item_tags WHERE item_id = ?1", params![id]).map_err(sql_err)?;
    for t in tags {
        let name = t.tag.trim();
        if name.is_empty() {
            continue;
        }
        let tid = tag_id(tx, library_id, name)?;
        tx.execute(
            "INSERT INTO item_tags(item_id, tag_id, type) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
            params![id, tid, t.r#type as i64],
        )
        .map_err(sql_err)?;
    }
    Ok(())
}

fn set_collections(tx: &Connection, library_id: i64, id: i64, keys: &[Key]) -> Result<()> {
    tx.execute("DELETE FROM collection_items WHERE item_id = ?1", params![id]).map_err(sql_err)?;
    for k in keys {
        let cid = collection_id(tx, library_id, k)?;
        tx.execute(
            "INSERT INTO collection_items(collection_id, item_id) VALUES (?1, ?2) \
             ON CONFLICT DO NOTHING",
            params![cid, id],
        )
        .map_err(sql_err)?;
    }
    Ok(())
}

/// Values kept outside the JSON blob purely so listing stays index-backed.
struct Denorm {
    sort_title: String,
    sort_creator: String,
    year: Option<i32>,
    fingerprint: String,
}

fn denorm(item: &Item) -> Denorm {
    Denorm {
        sort_title: text::normalize(item.title()),
        sort_creator: text::normalize(
            &item.creators.first().map(Creator::sort_name).unwrap_or_default(),
        ),
        year: item.year(),
        fingerprint: item.fingerprint(),
    }
}

fn validate(draft: &ItemDraft) -> Result<()> {
    let s = schema();
    if !s.has_type(&draft.item_type) {
        return Err(Error::invalid(format!("unknown itemType '{}'", draft.item_type)));
    }
    for c in &draft.creators {
        if c.name.is_none() && c.last_name.is_none() && c.first_name.is_none() {
            return Err(Error::invalid("creator must have a name"));
        }
    }
    Ok(())
}

fn insert(tx: &Connection, library_id: i64, draft: ItemDraft, version: i64) -> Result<(i64, Item)> {
    validate(&draft)?;

    let parent_id = match &draft.parent_key {
        Some(k) => Some(item_id(tx, library_id, k)?),
        None => None,
    };
    let key = match &draft.key {
        Some(k) => k.clone(),
        None => Key::generate(),
    };
    let tags = draft.tags.clone();
    let collections = draft.collections.clone();
    let item = draft.into_item(key, library_id, version);
    let d = denorm(&item);

    tx.execute(
        "INSERT INTO items(library_id, key, item_type, parent_id, fields, creators,
                           sort_title, sort_creator, year, fingerprint, deleted, version,
                           date_added, date_modified)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,?11,?12,?13)",
        params![
            library_id,
            item.key.as_str(),
            item.item_type,
            parent_id,
            serde_json::to_string(&item.fields)?,
            serde_json::to_string(&item.creators)?,
            d.sort_title,
            d.sort_creator,
            d.year,
            d.fingerprint,
            version,
            item.date_added,
            item.date_modified
        ],
    )
    .map_err(sql_err)?;

    let id = tx.last_insert_rowid();
    set_tags(tx, library_id, id, &tags)?;
    set_collections(tx, library_id, id, &collections)?;

    let mut stored = item;
    stored.tags = tags;
    stored.collections = collections;
    index::reindex(tx, id, &stored)?;
    Ok((id, stored))
}

fn load_one(conn: &Connection, library_id: i64, key: &Key) -> Result<(i64, Item)> {
    let sql = format!("SELECT {COLS} {FROM} WHERE i.library_id = ?1 AND i.key = ?2");
    let mut rows: Vec<(i64, Item)> = conn
        .prepare_cached(&sql)
        .map_err(sql_err)?
        .query_map(params![library_id, key.as_str()], map_row)
        .map_err(sql_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql_err)?;
    if rows.is_empty() {
        return Err(Error::not_found(format!("item {key}")));
    }
    hydrate(conn, &mut rows)?;
    Ok(rows.remove(0))
}

/// Merge a patch into the stored item. `null` in `fields` clears that field.
fn apply_patch(item: &mut Item, patch: ItemPatch) {
    if let Some(t) = patch.item_type {
        item.item_type = t;
    }
    if let Some(fields) = patch.fields {
        for (k, v) in fields {
            if v.is_null() {
                item.fields.remove(&k);
            } else {
                item.fields.insert(k, v);
            }
        }
    }
    if let Some(c) = patch.creators {
        item.creators = c;
    }
    if let Some(t) = patch.tags {
        item.tags = t;
    }
    if let Some(c) = patch.collections {
        item.collections = c;
    }
    if let Some(d) = patch.deleted {
        item.deleted = d;
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteItemRepository {
    db: Db,
}

impl SqliteItemRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Rebuild every derived search structure for a library from scratch.
    /// Safe to run at any time; the tables it touches are pure derivations.
    pub async fn rebuild_index(&self, library_id: i64) -> Result<u64> {
        const CHUNK: u32 = 500;
        let mut cursor = 0i64;
        let mut total = 0u64;

        // Clear first so removed items cannot linger in the index.
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                for table in ["items_fts", "items_trgm"] {
                    tx.execute(
                        &format!(
                            "DELETE FROM {table} WHERE rowid IN \
                             (SELECT id FROM items WHERE library_id = ?1)"
                        ),
                        params![library_id],
                    )
                    .map_err(sql_err)?;
                }
                tx.execute("DELETE FROM embed_queue WHERE library_id = ?1", params![library_id])
                    .map_err(sql_err)?;
                tx.execute("DELETE FROM item_vectors WHERE library_id = ?1", params![library_id])
                    .map_err(sql_err)?;
                tx.commit().map_err(sql_err)?;
                Ok(())
            })
            .await?;

        loop {
            let processed = self
                .db
                .call(move |c| {
                    let sql = format!(
                        "SELECT {COLS} {FROM} WHERE i.library_id = ?1 AND i.id > ?2 \
                         ORDER BY i.id LIMIT ?3"
                    );
                    let mut rows: Vec<(i64, Item)> = c
                        .prepare_cached(&sql)
                        .map_err(sql_err)?
                        .query_map(params![library_id, cursor, CHUNK], map_row)
                        .map_err(sql_err)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(sql_err)?;
                    if rows.is_empty() {
                        return Ok((0u64, cursor));
                    }
                    hydrate(c, &mut rows)?;
                    let last = rows.last().map(|(id, _)| *id).unwrap_or(cursor);

                    let tx = write_tx(c)?;
                    for (id, item) in &rows {
                        index::reindex(&tx, *id, item)?;
                    }
                    tx.commit().map_err(sql_err)?;
                    Ok((rows.len() as u64, last))
                })
                .await?;

            if processed.0 == 0 {
                break;
            }
            total += processed.0;
            cursor = processed.1;
        }
        Ok(total)
    }

    /// Resolve a collection filter to concrete ids, expanding descendants.
    fn resolve_collections(conn: &Connection, filter: &ItemFilter) -> Result<Option<Vec<i64>>> {
        let Some(key) = &filter.collection else { return Ok(None) };
        let root: Option<i64> = conn
            .query_row(
                "SELECT id FROM collections WHERE library_id = ?1 AND key = ?2",
                params![filter.library_id, key.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        let Some(root) = root else { return Ok(Some(Vec::new())) };
        if !filter.recursive {
            return Ok(Some(vec![root]));
        }
        let mut stmt = conn
            .prepare_cached(
                "WITH RECURSIVE sub(id) AS (
                    SELECT ?1
                    UNION ALL
                    SELECT c.id FROM collections c JOIN sub ON c.parent_id = sub.id
                 ) SELECT id FROM sub",
            )
            .map_err(sql_err)?;
        let ids = stmt
            .query_map(params![root], |r| r.get::<_, i64>(0))
            .map_err(sql_err)?
            .collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(sql_err)?;
        Ok(Some(ids))
    }
}

#[async_trait]
impl ItemRepository for SqliteItemRepository {
    async fn get(&self, library_id: i64, key: &Key) -> Result<Item> {
        let key = key.clone();
        self.db.call(move |c| load_one(c, library_id, &key).map(|(_, i)| i)).await
    }

    async fn get_many(&self, library_id: i64, keys: &[Key]) -> Result<Vec<Item>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        self.db
            .call(move |c| {
                // In runs: one placeholder per key, and callers pass whatever
                // the user selected.
                let mut rows: Vec<(i64, Item)> = Vec::with_capacity(keys.len());
                for run in crate::filter::chunks(&keys) {
                    let ph = placeholders(run.len());
                    let sql =
                        format!("SELECT {COLS} {FROM} WHERE i.library_id = ? AND i.key IN ({ph})");
                    let mut stmt = c.prepare_cached(&sql).map_err(sql_err)?;
                    let mut args: Vec<rusqlite::types::Value> =
                        vec![rusqlite::types::Value::Integer(library_id)];
                    args.extend(run.iter().map(|k| rusqlite::types::Value::Text(k.clone())));
                    let found = stmt
                        .query_map(params_from_iter(args), map_row)
                        .map_err(sql_err)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(sql_err)?;
                    rows.extend(found);
                }
                hydrate(c, &mut rows)?;
                Ok(rows.into_iter().map(|(_, i)| i).collect())
            })
            .await
    }

    async fn list(&self, query: &ItemQuery) -> Result<Page<Item>> {
        let query = query.clone().clamped();
        self.db
            .call(move |c| {
                let cols = Self::resolve_collections(c, &query.filter)?;
                let p = Predicate::build(&query.filter, cols.as_deref());

                let total: i64 = {
                    let sql = format!("SELECT count(*) FROM items i WHERE {}", p.sql);
                    c.prepare_cached(&sql)
                        .map_err(sql_err)?
                        .query_row(params_from_iter(p.params.iter()), |r| r.get(0))
                        .map_err(sql_err)?
                };

                // A deferred join: pick the page's ids from the index alone,
                // then fetch the columns for those rows only.
                //
                // The obvious shape joins the parent table for every row it
                // walks, including the fifty thousand an offset is about to
                // throw away. On a 100k library that was 95.7ms at offset
                // 50000; picking the ids from a covering index first makes it
                // 2.1ms. The outer ORDER BY re-sorts a hundred rows, which is
                // free, and is needed because `IN` does not preserve order.
                let order = order_by(query.sort, query.direction);
                let sql = format!(
                    "SELECT {COLS} {FROM} WHERE i.id IN ( \
                       SELECT i.id FROM items i WHERE {} {order} LIMIT ? OFFSET ?) {order}",
                    p.sql,
                );
                let mut args = p.params.clone();
                args.push(rusqlite::types::Value::Integer(query.limit as i64));
                args.push(rusqlite::types::Value::Integer(query.offset as i64));

                let mut rows: Vec<(i64, Item)> = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params_from_iter(args), map_row)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                hydrate(c, &mut rows)?;

                Ok(Page::new(
                    rows.into_iter().map(|(_, i)| i).collect(),
                    total,
                    query.offset,
                    query.limit,
                ))
            })
            .await
    }

    async fn children(&self, library_id: i64, parent: &Key) -> Result<Vec<Item>> {
        let parent = parent.clone();
        self.db
            .call(move |c| {
                let sql = format!(
                    "SELECT {COLS} {FROM} WHERE i.library_id = ?1 AND p.key = ?2 \
                     ORDER BY i.item_type, i.date_added"
                );
                let mut rows: Vec<(i64, Item)> = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id, parent.as_str()], map_row)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                hydrate(c, &mut rows)?;
                Ok(rows.into_iter().map(|(_, i)| i).collect())
            })
            .await
    }

    async fn attachments(
        &self,
        library_id: i64,
        limit: u32,
        offset: u32,
    ) -> Result<Page<(Item, Option<Item>)>> {
        self.db
            .call(move |c| {
                let total: i64 = c
                    .query_row(
                        "SELECT count(*) FROM items \
                         WHERE library_id = ?1 AND deleted = 0 AND item_type = 'attachment'",
                        params![library_id],
                        |r| r.get(0),
                    )
                    .map_err(sql_err)?;

                let sql = format!(
                    "SELECT {COLS} {FROM} \
                     WHERE i.library_id = ?1 AND i.deleted = 0 AND i.item_type = 'attachment' \
                     ORDER BY i.date_added DESC LIMIT ?2 OFFSET ?3"
                );
                let rows: Vec<(i64, Item)> = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id, limit, offset], map_row)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;

                // Deliberately *not* hydrated. Nothing that lists files wants
                // an attachment's tags or collections — the browser shows the
                // name, the parent, the address and the size; renaming wants
                // the parent's title, creators and year, and creators travel in
                // the row itself. Loading them anyway cost most of a rename
                // preview, and did it through an `IN (…)` of thirty thousand
                // placeholders — a hundred and sixty short of SQLite's limit,
                // so a slightly larger library would not have been slow, it
                // would have failed.

                // The parents in one pass. One query per attachment would be a
                // thousand round trips for a page nobody would wait for.
                let parents: Vec<Key> =
                    rows.iter().filter_map(|(_, i)| i.parent_key.clone()).collect();
                let mut by_key: std::collections::HashMap<String, Item> =
                    std::collections::HashMap::new();
                // In runs: one placeholder per parent, and a rename preview
                // asks about every attachment in the library at once.
                for run in crate::filter::chunks(&parents) {
                    // One placeholder per key. The library id has its own `?`
                    // in the statement; counting it here too is how this
                    // produced "got 3, needed 4" on the first real request.
                    let places = vec!["?"; run.len()].join(",");
                    let sql = format!(
                        "SELECT {COLS} {FROM} WHERE i.library_id = ? AND i.key IN ({places})"
                    );
                    let mut args: Vec<Box<dyn rusqlite::ToSql>> =
                        vec![Box::new(library_id)];
                    for key in run {
                        args.push(Box::new(key.to_string()));
                    }
                    let found: Vec<(i64, Item)> = c
                        .prepare(&sql)
                        .map_err(sql_err)?
                        .query_map(rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())), map_row)
                        .map_err(sql_err)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(sql_err)?;
                    for (_, item) in found {
                        by_key.insert(item.key.to_string(), item);
                    }
                }

                let items = rows
                    .into_iter()
                    .map(|(_, attachment)| {
                        let parent = attachment
                            .parent_key
                            .as_ref()
                            .and_then(|k| by_key.get(k.as_str()).cloned());
                        (attachment, parent)
                    })
                    .collect();

                Ok(Page { items, total, limit, offset })
            })
            .await
    }

    async fn create(&self, library_id: i64, draft: ItemDraft) -> Result<Item> {
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let version = bump_version(&tx, library_id)?;
                let (_, item) = insert(&tx, library_id, draft, version)?;
                tx.commit().map_err(sql_err)?;
                Ok(item)
            })
            .await
    }

    async fn create_many(
        &self,
        library_id: i64,
        drafts: Vec<ItemDraft>,
    ) -> Result<Vec<Result<Item>>> {
        self.db
            .call(move |c| {
                let mut tx = write_tx(c)?;
                let version = bump_version(&tx, library_id)?;
                let mut out = Vec::with_capacity(drafts.len());
                for draft in drafts {
                    // A savepoint per row keeps one bad draft from poisoning
                    // the batch.
                    let mut sp = tx.savepoint().map_err(sql_err)?;
                    match insert(&sp, library_id, draft, version) {
                        Ok((_, item)) => {
                            sp.commit().map_err(sql_err)?;
                            out.push(Ok(item));
                        }
                        Err(e) => {
                            let _ = sp.rollback();
                            out.push(Err(e));
                        }
                    }
                }
                tx.commit().map_err(sql_err)?;
                Ok(out)
            })
            .await
    }

    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: ItemPatch,
        if_version: Option<i64>,
    ) -> Result<Item> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let (id, mut item) = load_one(&tx, library_id, &key)?;

                if let Some(expected) = if_version {
                    if expected != item.version {
                        return Err(Error::VersionConflict {
                            expected,
                            current: item.version,
                        });
                    }
                }

                let tags_changed = patch.tags.is_some();
                let collections_changed = patch.collections.is_some();
                apply_patch(&mut item, patch);
                if !schema().has_type(&item.item_type) {
                    return Err(Error::invalid(format!("unknown itemType '{}'", item.item_type)));
                }

                let version = bump_version(&tx, library_id)?;
                item.version = version;
                item.date_modified = yk_core::now_ms();
                let d = denorm(&item);

                tx.execute(
                    "UPDATE items SET item_type=?1, fields=?2, creators=?3, sort_title=?4,
                         sort_creator=?5, year=?6, fingerprint=?7, deleted=?8, version=?9,
                         date_modified=?10 WHERE id=?11",
                    params![
                        item.item_type,
                        serde_json::to_string(&item.fields)?,
                        serde_json::to_string(&item.creators)?,
                        d.sort_title,
                        d.sort_creator,
                        d.year,
                        d.fingerprint,
                        i64::from(item.deleted),
                        version,
                        item.date_modified,
                        id
                    ],
                )
                .map_err(sql_err)?;

                if tags_changed {
                    set_tags(&tx, library_id, id, &item.tags)?;
                }
                if collections_changed {
                    set_collections(&tx, library_id, id, &item.collections)?;
                }
                index::reindex(&tx, id, &item)?;
                tx.commit().map_err(sql_err)?;
                Ok(item)
            })
            .await
    }

    async fn set_trashed(&self, library_id: i64, keys: &[Key], trashed: bool) -> Result<u64> {
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        if keys.is_empty() {
            return Ok(0);
        }
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let version = bump_version(&tx, library_id)?;

                // In runs, inside the one transaction. A statement binds one
                // value per key, and SQLite will not bind more than a few
                // thousand — "select all, move to trash" on a large library
                // failed outright before this, on a gesture that had always
                // worked.
                let mut n = 0u64;
                for run in crate::filter::chunks(&keys) {
                    let ph = placeholders(run.len());
                    let mut args: Vec<rusqlite::types::Value> = vec![
                        rusqlite::types::Value::Integer(i64::from(trashed)),
                        rusqlite::types::Value::Integer(version),
                        rusqlite::types::Value::Integer(yk_core::now_ms()),
                        rusqlite::types::Value::Integer(library_id),
                    ];
                    args.extend(run.iter().map(|k| rusqlite::types::Value::Text(k.clone())));
                    n += tx
                        .execute(
                            &format!(
                                "UPDATE items SET deleted=?1, version=?2, date_modified=?3 \
                                 WHERE library_id=?4 AND key IN ({ph})"
                            ),
                            params_from_iter(args),
                        )
                        .map_err(sql_err)? as u64;
                }

                // Keep the search index consistent with visibility.
                for k in &keys {
                    if let Ok(key) = Key::parse(k) {
                        if let Ok((id, item)) = load_one(&tx, library_id, &key) {
                            index::reindex(&tx, id, &item)?;
                        }
                    }
                }
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }

    async fn delete(&self, library_id: i64, keys: &[Key]) -> Result<u64> {
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        if keys.is_empty() {
            return Ok(0);
        }
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let version = bump_version(&tx, library_id)?;
                let mut removed = 0u64;
                for k in &keys {
                    let id: Option<i64> = tx
                        .query_row(
                            "SELECT id FROM items WHERE library_id=?1 AND key=?2",
                            params![library_id, k],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(sql_err)?;
                    let Some(id) = id else { continue };
                    index::remove(&tx, &[id])?;
                    tx.execute("DELETE FROM items WHERE id=?1", params![id]).map_err(sql_err)?;
                    tx.execute(
                        "INSERT INTO deletions(library_id, object_type, object_key, version, deleted_at)
                         VALUES (?1,'item',?2,?3,?4) ON CONFLICT DO UPDATE SET version=excluded.version",
                        params![library_id, k, version, yk_core::now_ms()],
                    )
                    .map_err(sql_err)?;
                    removed += 1;
                }
                tx.commit().map_err(sql_err)?;
                Ok(removed)
            })
            .await
    }

    async fn empty_trash(&self, library_id: i64) -> Result<u64> {
        let keys = self
            .db
            .call(move |c| {
                let mut stmt = c
                    .prepare("SELECT key FROM items WHERE library_id=?1 AND deleted=1")
                    .map_err(sql_err)?;
                let keys: Vec<String> = stmt
                    .query_map(params![library_id], |r| r.get(0))
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                Ok(keys)
            })
            .await?;
        let keys: Vec<Key> = keys.iter().filter_map(|k| Key::parse(k).ok()).collect();
        self.delete(library_id, &keys).await
    }

    async fn add_to_collection(
        &self,
        library_id: i64,
        collection: &Key,
        keys: &[Key],
    ) -> Result<u64> {
        let collection = collection.clone();
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let cid = collection_id(&tx, library_id, &collection)?;
                let version = bump_version(&tx, library_id)?;
                let mut n = 0u64;
                for k in &keys {
                    let key = Key::parse(k)?;
                    let id = item_id(&tx, library_id, &key)?;
                    tx.execute(
                        "INSERT INTO collection_items(collection_id, item_id) VALUES (?1,?2) \
                         ON CONFLICT DO NOTHING",
                        params![cid, id],
                    )
                    .map_err(sql_err)?;
                    tx.execute(
                        "UPDATE items SET version=?1, date_modified=?2 WHERE id=?3",
                        params![version, yk_core::now_ms(), id],
                    )
                    .map_err(sql_err)?;
                    n += 1;
                }
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }

    async fn remove_from_collection(
        &self,
        library_id: i64,
        collection: &Key,
        keys: &[Key],
    ) -> Result<u64> {
        let collection = collection.clone();
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let cid = collection_id(&tx, library_id, &collection)?;
                let version = bump_version(&tx, library_id)?;
                let mut n = 0u64;
                for k in &keys {
                    let key = Key::parse(k)?;
                    let id = item_id(&tx, library_id, &key)?;
                    n += tx
                        .execute(
                            "DELETE FROM collection_items WHERE collection_id=?1 AND item_id=?2",
                            params![cid, id],
                        )
                        .map_err(sql_err)? as u64;
                    tx.execute(
                        "UPDATE items SET version=?1, date_modified=?2 WHERE id=?3",
                        params![version, yk_core::now_ms(), id],
                    )
                    .map_err(sql_err)?;
                }
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }

    async fn find_by_fingerprint(
        &self,
        library_id: i64,
        fingerprints: &[String],
    ) -> Result<Vec<Item>> {
        if fingerprints.is_empty() {
            return Ok(Vec::new());
        }
        let fps = fingerprints.to_vec();
        self.db
            .call(move |c| {
                let ph = placeholders(fps.len());
                let sql = format!(
                    "SELECT {COLS} {FROM} WHERE i.library_id = ? AND i.deleted = 0 \
                     AND i.fingerprint IN ({ph})"
                );
                let mut args: Vec<rusqlite::types::Value> =
                    vec![rusqlite::types::Value::Integer(library_id)];
                args.extend(fps.iter().map(|f| rusqlite::types::Value::Text(f.clone())));
                let mut rows: Vec<(i64, Item)> = c
                    .prepare(&sql)
                    .map_err(sql_err)?
                    .query_map(params_from_iter(args), map_row)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                hydrate(c, &mut rows)?;
                Ok(rows.into_iter().map(|(_, i)| i).collect())
            })
            .await
    }

    async fn scan(
        &self,
        library_id: i64,
        after_rowid: i64,
        limit: u32,
    ) -> Result<(Vec<Item>, i64)> {
        self.db
            .call(move |c| {
                let sql = format!(
                    "SELECT {COLS} {FROM} WHERE i.library_id = ?1 AND i.id > ?2 \
                     ORDER BY i.id LIMIT ?3"
                );
                let mut rows: Vec<(i64, Item)> = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id, after_rowid, limit], map_row)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;
                hydrate(c, &mut rows)?;
                let last = rows.last().map(|(id, _)| *id).unwrap_or(after_rowid);
                Ok((rows.into_iter().map(|(_, i)| i).collect(), last))
            })
            .await
    }

    async fn count(&self, filter: &ItemFilter) -> Result<i64> {
        let filter = filter.clone();
        self.db
            .call(move |c| {
                let cols = Self::resolve_collections(c, &filter)?;
                let p = Predicate::build(&filter, cols.as_deref());
                let sql = format!("SELECT count(*) FROM items i WHERE {}", p.sql);
                c.prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_row(params_from_iter(p.params.iter()), |r| r.get(0))
                    .map_err(sql_err)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteSettingsRepository {
    db: Db,
}

impl SqliteSettingsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get(&self, key: &str) -> Result<Option<Value>> {
        let key = key.to_string();
        self.db
            .call(move |c| {
                let raw: Option<String> = c
                    .query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0))
                    .optional()
                    .map_err(sql_err)?;
                Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
            })
            .await
    }

    async fn set(&self, key: &str, value: &Value) -> Result<()> {
        let key = key.to_string();
        let raw = serde_json::to_string(value)?;
        self.db
            .call(move |c| {
                c.execute(
                    "INSERT INTO settings(key, value) VALUES (?1, ?2) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, raw],
                )
                .map_err(sql_err)?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let key = key.to_string();
        self.db
            .call(move |c| {
                c.execute("DELETE FROM settings WHERE key=?1", params![key]).map_err(sql_err)?;
                Ok(())
            })
            .await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<(String, Value)>> {
        let like = format!("{prefix}%");
        self.db
            .call(move |c| {
                let mut stmt = c
                    .prepare("SELECT key, value FROM settings WHERE key LIKE ?1 ORDER BY key")
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![like], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_err)?;
                Ok(rows
                    .into_iter()
                    .filter_map(|(k, v)| serde_json::from_str(&v).ok().map(|v| (k, v)))
                    .collect())
            })
            .await
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    use yk_core::query::{Direction, ItemQuery, SortField};

    /// What SQLite says it will do, rather than what it returns.
    fn plan(sql: &str, params: usize) -> String {
        let store = crate::Store::in_memory().unwrap();
        let conn = store.db().conn().unwrap();
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let bound: Vec<i64> = (0..params).map(|_| 1).collect();
        stmt.query_map(rusqlite::params_from_iter(bound), |r| r.get::<_, String>(3))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// The statement `list` builds, for a plain library page.
    fn list_sql(sort: SortField, direction: Direction) -> String {
        let query = ItemQuery {
            filter: yk_core::query::ItemFilter { library_id: 1, ..Default::default() },
            sort,
            direction,
            ..Default::default()
        };
        let p = Predicate::build(&query.filter, None);
        let order = order_by(query.sort, query.direction);
        format!(
            "SELECT {COLS} {FROM} WHERE i.id IN ( \
               SELECT i.id FROM items i WHERE {} {order} LIMIT ? OFFSET ?) {order}",
            p.sql,
        )
    }

    #[test]
    fn a_page_is_found_through_a_covering_index() {
        // The whole point of the deferred join. If the inner query stops being
        // covering, the parent join comes back for every row an offset is
        // about to discard, and a deep page goes from 2ms to 96ms with no
        // change in what it returns — invisible to every other test.
        let plan = plan(&list_sql(SortField::DateModified, Direction::Desc), 3);
        assert!(plan.contains("COVERING INDEX idx_items_modified"), "{plan}");
    }

    #[test]
    fn ties_are_broken_by_the_index_rather_than_by_sorting_the_library() {
        // Every list orders by a column and then by id. Without the id in the
        // index SQLite settles the ties in a temp b-tree, over every row it
        // walks: 88ms of the 96ms, on a page of a hundred.
        for (sort, name) in [
            (SortField::DateModified, "modified"),
            (SortField::DateAdded, "added"),
            (SortField::Title, "title"),
            (SortField::Creator, "creator"),
            (SortField::Year, "year"),
            (SortField::ItemType, "type"),
        ] {
            for direction in [Direction::Asc, Direction::Desc] {
                let plan = plan(&list_sql(sort, direction), 3);
                // Everything between the subquery and the parent join is the
                // inner query. The outer one sorts a hundred rows and may use
                // a temp b-tree freely.
                //
                // Matching on the exact wording is how the first version of
                // this test passed against the very regression it was written
                // for: SQLite says "LAST TERM OF ORDER BY" here, not "RIGHT
                // PART OF ORDER BY", so the assertion never fired.
                let inner = plan
                    .split("LIST SUBQUERY")
                    .nth(1)
                    .and_then(|rest| rest.split("SEARCH p").next())
                    .unwrap_or(&plan);
                assert!(
                    !inner.contains("TEMP B-TREE"),
                    "sorting by {name} {direction:?} settles ties in a temp b-tree: {plan}"
                );
            }
        }
    }
}
