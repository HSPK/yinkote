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
use crate::filter::{estimated_items, order_by, placeholders, Predicate, TagForm};
use crate::index;

const COLS: &str = "i.id, i.key, i.library_id, i.item_type, p.key, i.fields, i.creators, \
                    i.version, i.deleted, i.date_added, i.date_modified";
const FROM: &str = "FROM items i LEFT JOIN items p ON p.id = i.parent_id";

/// The parent's own columns, for the one caller that wants the whole parent
/// rather than just its key. The join in [`FROM`] already reaches the row, so
/// asking for these costs nothing beyond the bytes; fetching the parents in a
/// second pass — which is what this replaced — cost a third of a rename
/// preview and an `IN (…)` list the width of the library.
const PARENT_COLS: &str = "p.id, p.library_id, p.item_type, p.fields, p.creators, \
                           p.version, p.deleted, p.date_added, p.date_modified";

/// Read a parent item from the columns [`PARENT_COLS`] adds, starting at
/// `base`. `None` when the attachment is loose — the join is a `LEFT` one.
fn map_parent(row: &Row<'_>, base: usize, key: Option<Key>) -> rusqlite::Result<Option<Item>> {
    let Some(key) = key else { return Ok(None) };
    if row.get::<_, Option<i64>>(base)?.is_none() {
        return Ok(None);
    }
    let fields_raw: String = row.get(base + 3)?;
    let creators_raw: String = row.get(base + 4)?;
    Ok(Some(Item {
        key,
        library_id: row.get(base + 1)?,
        item_type: row.get(base + 2)?,
        parent_key: None,
        fields: serde_json::from_str(&fields_raw).unwrap_or_default(),
        creators: serde_json::from_str(&creators_raw).unwrap_or_default(),
        tags: Vec::new(),
        collections: Vec::new(),
        version: row.get(base + 5)?,
        deleted: row.get::<_, i64>(base + 6)? != 0,
        attachments: Vec::new(),
        date_added: row.get(base + 7)?,
        date_modified: row.get(base + 8)?,
    }))
}

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
        attachments: Vec::new(),
        date_added: row.get(9)?,
        date_modified: row.get(10)?,
    };
    Ok((id, item))
}

/// The statement a page of items is read with.
///
/// One function so the plan assertions can question the statement that actually
/// runs. They used to rebuild it by hand, which is fine until the two drift —
/// and they drifted the moment an index hint was added, leaving a test that
/// proved something true about a query nobody executes.
///
/// A deferred join: pick the page's ids from an index alone, then fetch the
/// columns for those rows only. The obvious shape joins the parent table for
/// every row it walks, including the fifty thousand an offset is about to throw
/// away — 95.7ms at offset 50000 against 2.1ms. The outer `ORDER BY` re-sorts a
/// hundred rows, which is free, and is needed because `IN` does not preserve
/// order.
fn page_sql(p: &Predicate, sort: SortField, direction: Direction) -> String {
    let order = order_by(sort, direction);
    // Name the index for a plain browse. See `filter::sort_index`: one unrelated
    // index with the same leading columns was enough to make the planner throw
    // the order away and re-sort the library — 9ms to 69ms, same results.
    // Also named for the probe form, and for the same reason: its whole point
    // is to read the sort order and stop at a full page, which it can only do
    // if the walk is the index that already holds that order.
    let hint = if p.base_only || p.tags_only {
        format!("INDEXED BY {}", crate::filter::sort_index(sort))
    } else {
        String::new()
    };
    format!(
        "SELECT {COLS} {FROM} WHERE i.id IN ( \
           SELECT i.id FROM items i {hint} WHERE {} {order} LIMIT ? OFFSET ?) {order}",
        p.sql,
    )
}

/// Fill in tags and collections for a page of items using two batched queries
/// instead of N+1.
/// Look items up by the identifier a publisher gave them.
///
/// `INDEXED BY` is load-bearing, and this is the third time on this table:
/// left alone the planner searched `idx_items_year` — a predicate matching
/// the whole library — and scanned it looking for a fingerprint. 66 ms
/// against 0.15 ms on a large library, for a duplicate check that runs on
/// every item added. Identical results, so only a plan assertion catches it.
pub fn fingerprint_sql(placeholders: &str) -> String {
    format!(
        "SELECT {COLS} FROM items i INDEXED BY idx_items_fingerprint \
         LEFT JOIN items p ON p.id = i.parent_id \
         WHERE i.library_id = ? AND i.fingerprint IN ({placeholders}) AND i.deleted = 0"
    )
}

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
        drop(cur);
        drop(stmt);

        // What each row has attached. One query for the page, driven by
        // `idx_items_parent`; the alternative — asking per row — is the same
        // mistake the tag and collection passes above already avoid.
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT a.parent_id, json_extract(a.fields, '$.contentType'),                         json_extract(a.fields, '$.linkMode')                  FROM items a                  WHERE a.parent_id IN ({ph}) AND a.deleted = 0 AND a.item_type = 'attachment'"
            ))
            .map_err(sql_err)?;
        let mut cur = stmt.query(params_from_iter(run.iter())).map_err(sql_err)?;
        while let Some(r) = cur.next().map_err(sql_err)? {
            let id: i64 = r.get(0).map_err(sql_err)?;
            let Some(&idx) = by_id.get(&id) else { continue };
            let content_type: Option<String> = r.get(1).map_err(sql_err)?;
            let link_mode: Option<String> = r.get(2).map_err(sql_err)?;
            let kind = AttachmentKind::classify(content_type.as_deref(), link_mode.as_deref());
            let marks = &mut rows[idx].1.attachments;
            if !marks.contains(&kind) {
                marks.push(kind);
            }
        }
    }

    // A stable, meaningful order: a row with a PDF and two images should lead
    // with the PDF however the rows happened to come back.
    for (_, item) in rows.iter_mut() {
        if item.attachments.len() > 1 {
            item.attachments
                .sort_by_key(|k| AttachmentKind::ORDER.iter().position(|o| o == k).unwrap_or(9));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Write helpers
// ---------------------------------------------------------------------------

/// Apply one patch to one item, inside a transaction that is already open and
/// at a version that has already been taken.
///
/// Shared by the single and the batch update so that the two cannot drift:
/// everything here — the version check, the denormalised sort columns, the tag
/// and collection replacement, the reindex — is part of what "an item changed"
/// means, and a batch that skipped any of it would corrupt the library quietly.
fn apply_update(
    tx: &Connection,
    library_id: i64,
    key: &Key,
    patch: ItemPatch,
    if_version: Option<i64>,
    version: i64,
) -> Result<Item> {
    let (id, mut item) = load_one(tx, library_id, key)?;

    if let Some(expected) = if_version {
        if expected != item.version {
            return Err(Error::VersionConflict { expected, current: item.version });
        }
    }

    let tags_changed = patch.tags.is_some();
    let collections_changed = patch.collections.is_some();
    apply_patch(&mut item, patch);
    if !schema().has_type(&item.item_type) {
        return Err(Error::invalid(format!("unknown itemType '{}'", item.item_type)));
    }

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
        set_tags(tx, library_id, id, &item.tags)?;
    }
    if collections_changed {
        set_collections(tx, library_id, id, &item.collections)?;
    }
    index::reindex(tx, id, &item)?;
    Ok(item)
}

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

/// The library-wide duplicate scan, by identifier.
///
/// `INDEXED BY` for the usual reason (see `tests/fingerprint_plans.rs`): read
/// in fingerprint order the grouping is already done, where driving from
/// `parent_id IS NULL` sorts the whole library into groups instead — 62ms
/// against 52ms on the 130k benchmark library.
///
/// The empty fingerprint and `t:|a:|y:` are excluded because an item with no
/// title, author or year matches every other such item: true by the letter of
/// it, and useless, since there is nothing to compare.
pub const DUPLICATE_SCAN_SQL: &str = "SELECT group_concat(id) FROM items \
     INDEXED BY idx_items_fingerprint \
     WHERE library_id = ?1 AND deleted = 0 AND parent_id IS NULL \
       AND fingerprint <> '' AND fingerprint <> 't:|a:|y:' \
     GROUP BY fingerprint HAVING count(*) > 1 \
     ORDER BY count(*) DESC LIMIT ?2";

/// The same scan, by what is written on the paper.
///
/// Both scans are needed, and a smoke check is what proved it. A fingerprint
/// prefers an identifier, so a record with a DOI and a record without one never
/// share a fingerprint however identical they are — and "one copy imported from
/// the publisher, one typed by hand" is the commonest duplicate there is. That
/// pair is only caught by comparing the title, the first author and the year,
/// which are already denormalised for sorting.
///
/// Deliberately unhinted: `INDEXED BY idx_items_title` was measured and made it
/// worse (142ms against 108ms), because grouping on three columns still has to
/// sort within each title. 160ms for both scans on a 130k library, for a screen
/// that is opened on purpose.
pub const DUPLICATE_TITLE_SCAN_SQL: &str = "SELECT group_concat(id) FROM items \
     WHERE library_id = ?1 AND deleted = 0 AND parent_id IS NULL AND sort_title <> '' \
     GROUP BY sort_title, sort_creator, year HAVING count(*) > 1 \
     ORDER BY count(*) DESC LIMIT ?2";

/// Merge id sets that share a member.
///
/// Two records matched by DOI and two matched by title are one group of three
/// when they have a record in common; showing that record in two groups would
/// invite somebody to merge it into two different masters.
fn coalesce(sets: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
    let mut out: Vec<Vec<i64>> = Vec::with_capacity(sets.len());
    for set in sets {
        // Everything this set touches, folded together with it.
        let mut merged = set;
        let mut i = 0;
        while i < out.len() {
            if out[i].iter().any(|id| merged.contains(id)) {
                let taken = out.swap_remove(i);
                for id in taken {
                    if !merged.contains(&id) {
                        merged.push(id);
                    }
                }
                // `swap_remove` moved a new candidate into this slot.
                i = 0;
            } else {
                i += 1;
            }
        }
        merged.sort_unstable();
        out.push(merged);
    }
    out
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

    // A field nobody recognises is refused rather than stored.
    //
    // `Item` flattens its fields onto the wire, so a client that posts the
    // obvious-looking `{"fields": {"title": "..."}}` used to get an item
    // carrying a field literally called "fields" and no title at all — stored
    // happily, shown nowhere, reported never. Being told is the whole point.
    let unknown = s.validate(&draft.item_type, &draft.fields)?;
    if !unknown.is_empty() {
        return Err(Error::invalid(format!(
            "unknown field{} for {}: {}",
            if unknown.len() == 1 { "" } else { "s" },
            draft.item_type,
            unknown.join(", ")
        )));
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
/// Run one step of a background rebuild, waiting out a busy database.
///
/// Every step here is idempotent — clearing rows that are already gone, or
/// reindexing a batch that was just reindexed, costs time and changes nothing —
/// so losing the write lock is a reason to wait, not a reason to abandon a
/// half-finished index.
///
/// It needs this because it competes with the program it runs inside: a rebuild
/// makes a large write-ahead log, the log invites a checkpoint, and a
/// checkpoint takes the database exclusively for longer than any single busy
/// timeout. Small transactions and yielding are not enough on their own; they
/// keep the *interactive* writes fast, and this keeps the rebuild alive.
async fn retry_busy<T, F, Fut>(mut step: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    const ATTEMPTS: usize = 6;
    let mut wait = std::time::Duration::from_millis(200);
    for attempt in 1..=ATTEMPTS {
        match step().await {
            Err(e) if attempt < ATTEMPTS && Self::is_busy(&e) => {
                tokio::time::sleep(wait).await;
                wait *= 2;
            }
            other => return other,
        }
    }
    unreachable!("the loop returns on the last attempt")
}

fn is_busy(e: &Error) -> bool {
    // Matched on the message because that is what the storage layer preserves;
    // the alternative is threading a SQLite error kind through every wrapper
    // for one caller.
    e.to_string().contains("database is locked")
}

    pub async fn rebuild_index(&self, library_id: i64) -> Result<u64> {
        const CHUNK: u32 = 500;
        let mut cursor = 0i64;
        let mut total = 0u64;

        // Clear first so removed items cannot linger in the index — in
        // batches, and this is the reason: emptying two full-text tables and
        // the vectors for a hundred thousand items in one transaction holds the
        // write lock for eighteen seconds. Nothing noticed while a rebuild was
        // something the caller waited for; the moment it became a background
        // job, every other write in the program failed with "database is
        // locked" for as long as it ran.
        let mut clear_from = 0i64;
        loop {
            let db = self.db.clone();
            let last = Self::retry_busy(|| {
                let db = db.clone();
                async move {
                    db.call(move |c| {
                    let ids: Vec<i64> = c
                        .prepare_cached(
                            "SELECT id FROM items WHERE library_id = ?1 AND id > ?2 \
                             ORDER BY id LIMIT ?3",
                        )
                        .map_err(sql_err)?
                        .query_map(params![library_id, clear_from, CHUNK], |r| r.get(0))
                        .map_err(sql_err)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(sql_err)?;
                    let Some(&last) = ids.last() else { return Ok(None) };

                    let ph = placeholders(ids.len());
                    let tx = write_tx(c)?;
                    for table in ["items_fts", "items_trgm"] {
                        tx.execute(
                            &format!("DELETE FROM {table} WHERE rowid IN ({ph})"),
                            params_from_iter(ids.iter()),
                        )
                        .map_err(sql_err)?;
                    }
                    tx.execute(
                        &format!("DELETE FROM embed_queue WHERE item_id IN ({ph})"),
                        params_from_iter(ids.iter()),
                    )
                    .map_err(sql_err)?;
                    tx.execute(
                        &format!("DELETE FROM item_vectors WHERE item_id IN ({ph})"),
                        params_from_iter(ids.iter()),
                    )
                    .map_err(sql_err)?;
                    tx.commit().map_err(sql_err)?;
                    Ok(Some(last))
                    })
                    .await
                }
            })
            .await?;
            match last {
                Some(id) => clear_from = id,
                None => break,
            }
            // Let a waiting writer have the lock before taking it again. Small
            // transactions are not enough on their own: a loop that reacquires
            // immediately still starves everyone else, and here it starved
            // *itself* — the rebuild lost the race often enough to exhaust its
            // busy timeout and fail. The embedding worker learned this first.
            tokio::task::yield_now().await;
        }

        loop {
            let db = self.db.clone();
            let processed = Self::retry_busy(|| {
                let db = db.clone();
                async move {
                    db.call(move |c| {
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
                    .await
                }
            })
            .await?;

            if processed.0 == 0 {
                break;
            }
            total += processed.0;
            cursor = processed.1;
            // As above: a background job holds the write lock in turns, not in
            // one long run.
            tokio::task::yield_now().await;
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

                // Which way to write the tag filter is a cost decision, and
                // `total` is the number it turns on — which is why the count
                // runs first. See `filter::should_walk`.
                let p = if p.tags_only
                    && crate::filter::should_walk(
                        total,
                        (query.offset + query.limit) as i64,
                        estimated_items(c),
                    ) {
                    Predicate::build_with(&query.filter, cols.as_deref(), TagForm::Probe)
                } else {
                    p
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
                let sql = page_sql(&p, query.sort, query.direction);
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

    async fn children_of(&self, library_id: i64, parents: &[Key]) -> Result<Vec<Item>> {
        if parents.is_empty() {
            return Ok(Vec::new());
        }
        let parents: Vec<String> = parents.iter().map(|k| k.to_string()).collect();
        self.db
            .call(move |c| {
                // In runs, because "select all and empty the trash" passes the
                // whole library through here and a statement binds one value
                // per key.
                let mut out = Vec::new();
                for run in crate::filter::chunks(&parents) {
                    let ph = placeholders(run.len());
                    let sql = format!(
                        "SELECT {COLS} {FROM} WHERE i.library_id = ? AND p.key IN ({ph})"
                    );
                    let mut args: Vec<rusqlite::types::Value> =
                        vec![rusqlite::types::Value::Integer(library_id)];
                    args.extend(run.iter().map(|k| rusqlite::types::Value::Text(k.clone())));
                    let found: Vec<(i64, Item)> = c
                        .prepare_cached(&sql)
                        .map_err(sql_err)?
                        .query_map(params_from_iter(args), map_row)
                        .map_err(sql_err)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(sql_err)?;
                    out.extend(found.into_iter().map(|(_, i)| i));
                }
                Ok(out)
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

                // The attachment and its parent in one statement. The join is
                // already there for `parent_key`; taking the rest of the parent
                // from it turns two passes into one.
                let sql = format!(
                    "SELECT {COLS}, {PARENT_COLS} {FROM} \
                     WHERE i.library_id = ?1 AND i.deleted = 0 AND i.item_type = 'attachment' \
                     ORDER BY i.date_added DESC LIMIT ?2 OFFSET ?3"
                );
                let items: Vec<(Item, Option<Item>)> = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id, limit, offset], |row| {
                        let (_, attachment) = map_row(row)?;
                        // `map_row` reads eleven columns; the parent's begin
                        // straight after, which is what `PARENT_COLS` appends.
                        let parent = map_parent(row, 11, attachment.parent_key.clone())?;
                        Ok((attachment, parent))
                    })
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql_err)?;

                // Deliberately *not* hydrated. Nothing that lists files wants
                // an attachment's tags or collections — the browser shows the
                // name, the parent, the address and the size; renaming wants
                // the parent's title, creators and year, and those travel in
                // the row itself. Loading them anyway cost most of a rename
                // preview, and did it through an `IN (…)` of thirty thousand
                // placeholders — a hundred and sixty short of SQLite's limit,
                // so a slightly larger library would not have been slow, it
                // would have failed.

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
                let version = bump_version(&tx, library_id)?;
                let item = apply_update(&tx, library_id, &key, patch, if_version, version)?;
                tx.commit().map_err(sql_err)?;
                Ok(item)
            })
            .await
    }

    async fn update_many(
        &self,
        library_id: i64,
        patches: Vec<(Key, ItemPatch)>,
    ) -> Result<Vec<Result<Item>>> {
        if patches.is_empty() {
            return Ok(Vec::new());
        }
        self.db
            .call(move |c| {
                // One transaction and one version for the batch. Renaming a
                // library's files one item at a time cost 3.9ms each — two
                // minutes for thirty thousand files — and left thirty thousand
                // version bumps behind, which is a sync delta nobody wants to
                // send. A bulk edit is one thing happening, so it is one
                // version.
                let mut tx = write_tx(c)?;
                let version = bump_version(&tx, library_id)?;
                let mut out = Vec::with_capacity(patches.len());
                for (key, patch) in patches {
                    // A savepoint per row, as in `create_many`: one item that
                    // has since been deleted must not lose the other changes.
                    let mut sp = tx.savepoint().map_err(sql_err)?;
                    match apply_update(&sp, library_id, &key, patch, None, version) {
                        Ok(item) => {
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

    async fn duplicate_groups(&self, library_id: i64, limit: u32) -> Result<Vec<Vec<Item>>> {
        self.db
            .call(move |c| {
                // Two ways of being the same paper, and an item is in a group
                // if it matches either. They overlap — three records where two
                // share a DOI and two share a title are one group of three, not
                // two groups of two — so the id sets are merged rather than
                // concatenated.
                let mut sets: Vec<Vec<i64>> = Vec::new();
                for sql in [DUPLICATE_SCAN_SQL, DUPLICATE_TITLE_SCAN_SQL] {
                    let mut stmt = c.prepare_cached(sql).map_err(sql_err)?;
                    let rows = stmt
                        .query_map(params![library_id, limit], |r| r.get::<_, String>(0))
                        .map_err(sql_err)?;
                    for row in rows {
                        let ids: Vec<i64> = row
                            .map_err(sql_err)?
                            .split(',')
                            .filter_map(|id| id.parse().ok())
                            .collect();
                        if ids.len() > 1 {
                            sets.push(ids);
                        }
                    }
                }
                let sets = coalesce(sets);
                if sets.is_empty() {
                    return Ok(Vec::new());
                }

                // Every member of every group in one pass. Tags, collections
                // and attachments are what a person compares when deciding
                // which copy to keep, so the rows are hydrated.
                let ids: Vec<i64> = sets.iter().flatten().copied().collect();
                let mut by_id: HashMap<i64, Item> = HashMap::new();
                for run in crate::filter::chunks(&ids) {
                    let ph = placeholders(run.len());
                    let sql = format!("SELECT {COLS} {FROM} WHERE i.id IN ({ph})");
                    let mut rows: Vec<(i64, Item)> = c
                        .prepare(&sql)
                        .map_err(sql_err)?
                        .query_map(params_from_iter(run.iter()), map_row)
                        .map_err(sql_err)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(sql_err)?;
                    hydrate(c, &mut rows)?;
                    by_id.extend(rows);
                }

                Ok(sets
                    .into_iter()
                    .take(limit as usize)
                    .map(|set| set.iter().filter_map(|id| by_id.remove(id)).collect::<Vec<_>>())
                    .filter(|group: &Vec<Item>| group.len() > 1)
                    .collect())
            })
            .await
    }

    async fn merge(&self, library_id: i64, master: &Key, others: &[Key]) -> Result<Item> {
        let master = master.clone();
        let others: Vec<Key> = others.iter().filter(|k| **k != master).cloned().collect();
        if others.is_empty() {
            return self.get(library_id, &master).await;
        }
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let version = bump_version(&tx, library_id)?;
                let (master_id, mut kept) = load_one(&tx, library_id, &master)?;

                for key in &others {
                    let (id, other) = load_one(&tx, library_id, key)?;

                    // Attachments and notes move across. They are the reason
                    // merging is worth doing: the copy somebody is about to
                    // discard is often the one with the PDF on it.
                    tx.execute(
                        "UPDATE items SET parent_id = ?1, version = ?2, date_modified = ?3 \
                         WHERE parent_id = ?4",
                        params![master_id, version, yk_core::now_ms(), id],
                    )
                    .map_err(sql_err)?;

                    for tag in &other.tags {
                        if !kept.tags.iter().any(|t| t.tag == tag.tag) {
                            kept.tags.push(tag.clone());
                        }
                    }
                    for collection in &other.collections {
                        if !kept.collections.contains(collection) {
                            kept.collections.push(collection.clone());
                        }
                    }
                    // Fields are filled in, never overwritten: the master is
                    // the copy the user chose to keep, and the merge is only
                    // allowed to supply what it was missing — a DOI on one
                    // record and an abstract on the other is the usual case.
                    for (name, value) in &other.fields {
                        let blank = kept
                            .fields
                            .get(name)
                            .map(|v| v.as_str().is_some_and(|s| s.trim().is_empty()))
                            .unwrap_or(true);
                        if blank && !value.is_null() {
                            kept.fields.insert(name.clone(), value.clone());
                        }
                    }

                    // Soft, so the whole merge can be undone from the trash.
                    // Losing a record to a merge is the one mistake in a
                    // reference manager nobody can put right by hand.
                    //
                    // The duplicate keeps its own relations and citations; they
                    // hang off a trashed item, and every query that reads them
                    // filters on `deleted = 0`.
                    tx.execute(
                        "UPDATE items SET deleted = 1, version = ?1, date_modified = ?2 \
                         WHERE id = ?3",
                        params![version, yk_core::now_ms(), id],
                    )
                    .map_err(sql_err)?;
                }

                kept.version = version;
                kept.date_modified = yk_core::now_ms();
                let d = denorm(&kept);
                tx.execute(
                    "UPDATE items SET fields = ?1, sort_title = ?2, sort_creator = ?3, \
                         year = ?4, fingerprint = ?5, version = ?6, date_modified = ?7 \
                     WHERE id = ?8",
                    params![
                        serde_json::to_string(&kept.fields)?,
                        d.sort_title,
                        d.sort_creator,
                        d.year,
                        d.fingerprint,
                        version,
                        kept.date_modified,
                        master_id
                    ],
                )
                .map_err(sql_err)?;
                set_tags(&tx, library_id, master_id, &kept.tags)?;
                set_collections(&tx, library_id, master_id, &kept.collections)?;
                index::reindex(&tx, master_id, &kept)?;
                tx.commit().map_err(sql_err)?;
                Ok(kept)
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
                let sql = fingerprint_sql(&ph);
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
    ///
    /// Against a database with rows and statistics. An empty one is a different
    /// planner: with no `sqlite_stat1` it falls back to heuristics and happens
    /// to choose well, so an assertion here passed all the way through a change
    /// that made a plain browse seven times slower in production. See
    /// `docs/16` §3.66.
    fn plan(sql: &str, params: usize) -> String {
        let store = seeded();
        let conn = store.db().conn().unwrap();
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let bound: Vec<i64> = (0..params).map(|_| 1).collect();
        stmt.query_map(rusqlite::params_from_iter(bound), |r| r.get::<_, String>(3))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A library with enough in it, and enough shape, for the planner to choose
    /// the way it would in front of a real one.
    fn seeded() -> crate::Store {
        let store = crate::Store::in_memory().unwrap();
        {
            let conn = store.db().conn().unwrap();
            conn.execute_batch("BEGIN").unwrap();
            {
                let mut insert = conn
                    .prepare(
                        "INSERT INTO items(library_id, key, item_type, parent_id, sort_title, \
                                           sort_creator, year, fingerprint, attachment_rank, \
                                           date_added, date_modified) \
                         VALUES (1, ?1, ?2, ?3, ?1, ?1, 2020, ?1, ?4, ?5, ?5)",
                    )
                    .unwrap();
                for i in 0..8000 {
                    let top = i % 4 != 0;
                    insert
                        .execute(rusqlite::params![
                            format!("K{i:07}"),
                            if top { "journalArticle" } else { "attachment" },
                            if top { None } else { Some(1_i64) },
                            i64::from(i % 5 == 0),
                            i as i64,
                        ])
                        .unwrap();
                }
            }
            conn.execute_batch("COMMIT; ANALYZE;").unwrap();
        }
        store
    }

    /// The statement `list` builds, for a plain library page — the same
    /// builder, not a copy of it.
    fn list_sql(sort: SortField, direction: Direction) -> String {
        let filter = yk_core::query::ItemFilter { library_id: 1, ..Default::default() };
        page_sql(&Predicate::build(&filter, None), sort, direction)
    }

    /// Browsing must never sort the library.
    ///
    /// Every sort field has an index that already holds its order; using it
    /// means reading a hundred rows and stopping. Failing to means reading a
    /// hundred and thirty thousand and sorting them for the same answer, which
    /// is what happened when `idx_items_attachment` turned up with the same
    /// leading columns as the index this depends on.
    ///
    /// Honest about its reach, like `tests/fingerprint_plans.rs`: at any size
    /// this can seed, SQLite chooses correctly with or without the hint, so
    /// removing the hint does *not* make this fail. It guards the shape — that
    /// each order still has an index and the statement still names it — and
    /// `scripts/bench.mjs` guards the production plan, where it now fails the
    /// run rather than printing a number nobody compares.
    #[test]
    fn every_browse_order_comes_from_an_index() {
        for sort in [
            SortField::DateModified,
            SortField::DateAdded,
            SortField::Title,
            SortField::Creator,
            SortField::Year,
            SortField::ItemType,
            SortField::Attachment,
        ] {
            for direction in [Direction::Desc, Direction::Asc] {
                let sql = list_sql(sort, direction);
                // library id, limit, offset.
                let plan = plan(&sql, 3);

                assert!(
                    plan.contains(crate::filter::sort_index(sort)),
                    "{sort:?}/{direction:?} does not read the index that holds its order:\n  {plan}"
                );
                // Exactly one sort is expected and free: the outer statement
                // putting the hundred rows it fetched back in order, which `IN`
                // does not preserve. A second one is the library being sorted,
                // which is the regression this test exists for — it went from
                // one to two the day an index with the same leading columns
                // turned up, and from 9ms to 69ms with it.
                assert!(
                    plan.matches("TEMP B-TREE").count() <= 1,
                    "{sort:?}/{direction:?} sorts the library instead of reading it in order:\n  {plan}"
                );
            }
        }
    }

    #[test]
    fn a_collection_page_is_driven_by_its_memberships() {
        // The natural phrasing — `EXISTS (… WHERE ci.item_id = i.id)` — makes
        // SQLite walk the whole library and probe memberships per row: 41.8ms
        // against 2.0ms, with identical results. Only the plan shows it.
        let query = ItemQuery {
            filter: yk_core::query::ItemFilter { library_id: 1, ..Default::default() },
            ..Default::default()
        };
        let p = Predicate::build(&query.filter, Some(&[7]));
        let order = order_by(query.sort, query.direction);
        let sql = format!(
            "SELECT {COLS} {FROM} WHERE i.id IN (                SELECT i.id FROM items i WHERE {} {order} LIMIT ? OFFSET ?) {order}",
            p.sql,
        );

        let plan = plan(&sql, 4);
        assert!(plan.contains("SEARCH ci USING"), "memberships must lead: {plan}");
        assert!(
            !plan.contains("CORRELATED"),
            "a correlated subquery here means one probe per library row: {plan}"
        );
    }

    #[test]
    fn a_duplicate_check_seeks_by_fingerprint() {
        // Third time on this table. Left to itself the planner searched
        // `idx_items_year` — a predicate matching the whole library — and
        // scanned it looking for a fingerprint: 66ms against 0.15ms, on the
        // check that runs every time an item is added.
        let plan = plan(&fingerprint_sql("?,?"), 3);
        assert!(plan.contains("idx_items_fingerprint"), "{plan}");
        assert!(!plan.contains("idx_items_year"), "{plan}");
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

/// The two ways a tag filter can be written must return the same rows.
///
/// The optimisation is only sound if the choice is invisible, so this is the
/// test that matters most: it runs both plans over the same library and
/// compares them page by page. Everything else here is about speed; this is
/// about being allowed to choose at all.
#[cfg(test)]
mod tag_form_equivalence {
    use super::*;
    use yk_core::model::ItemTag;
    use yk_core::query::{Direction, ItemFilter, ItemQuery, SortField};

    async fn library() -> (crate::Store, i64) {
        let store = crate::Store::in_memory().unwrap();
        let lib = store.default_library;
        for i in 0..200 {
            let mut draft = yk_core::model::ItemDraft::new("journalArticle")
                .with_field("title", format!("Paper {i:03}"))
                .with_field("date", format!("{}", 1990 + i % 30));
            // A common tag, a rare one, and one nothing carries — the three
            // cases the rule has to get right.
            let mut tags = vec![ItemTag::manual("common")];
            if i % 97 == 0 {
                tags.push(ItemTag::manual("rare"));
            }
            if i % 2 == 0 {
                tags.push(ItemTag::manual("even"));
            }
            draft.tags = tags;
            store.items.create(lib, draft).await.unwrap();
        }
        (store, lib)
    }

    async fn keys(store: &crate::Store, lib: i64, tags: &[&str], form: TagForm) -> Vec<String> {
        let filter = ItemFilter {
            library_id: lib,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        };
        let query = ItemQuery {
            filter: filter.clone(),
            sort: SortField::DateModified,
            direction: Direction::Desc,
            limit: 25,
            offset: 0,
        };
        let cols: Option<Vec<i64>> = None;
        let p = Predicate::build_with(&filter, cols.as_deref(), form);
        let sql = page_sql(&p, query.sort, query.direction);
        let mut args = p.params.clone();
        args.push(rusqlite::types::Value::Integer(query.limit as i64));
        args.push(rusqlite::types::Value::Integer(query.offset as i64));

        let conn = store.db().conn().unwrap();
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        rows
    }

    #[tokio::test]
    async fn both_forms_return_the_same_page() {
        let (store, lib) = library().await;
        for tags in [&["common"][..], &["rare"][..], &["nobody-has-this"][..], &["common", "even"][..]] {
            let materialised = keys(&store, lib, tags, TagForm::Materialise).await;
            let probed = keys(&store, lib, tags, TagForm::Probe).await;
            assert_eq!(materialised, probed, "the two plans disagreed for {tags:?}");
        }
    }

    #[tokio::test]
    async fn an_unknown_tag_matches_nothing_either_way() {
        // The inner scalar subquery yields NULL for a tag that does not exist,
        // and NULL behaves differently in `IN` and in `=`. Both must still say
        // "no rows" rather than "every row".
        let (store, lib) = library().await;
        assert!(keys(&store, lib, &["nobody-has-this"], TagForm::Materialise).await.is_empty());
        assert!(keys(&store, lib, &["nobody-has-this"], TagForm::Probe).await.is_empty());
    }

    #[tokio::test]
    async fn every_sort_order_agrees_too() {
        // The probe form names the sort index, so a disagreement would show up
        // as rows in the wrong order rather than the wrong rows.
        let (store, lib) = library().await;
        let filter = ItemFilter { library_id: lib, tags: vec!["even".into()], ..Default::default() };
        for sort in [SortField::Title, SortField::DateAdded, SortField::Year, SortField::Creator] {
            let cols: Option<Vec<i64>> = None;
            let read = |form| {
                let p = Predicate::build_with(&filter, cols.as_deref(), form);
                let sql = page_sql(&p, sort, Direction::Asc);
                let mut args = p.params.clone();
                args.push(rusqlite::types::Value::Integer(25));
                args.push(rusqlite::types::Value::Integer(0));
                let conn = store.db().conn().unwrap();
                let mut stmt = conn.prepare(&sql).unwrap();
                let out: Vec<String> = stmt
                    .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(1))
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                out
            };
            assert_eq!(read(TagForm::Materialise), read(TagForm::Probe), "sort {sort:?}");
        }
    }
}

/// The bounded cardinality probe, which decides a plan without counting.
#[cfg(test)]
mod probe_tests {
    use yk_core::model::{ItemDraft, ItemTag};
    use yk_core::query::ItemFilter;

    async fn library(common: usize, rare: usize) -> crate::Store {
        let store = crate::Store::in_memory().unwrap();
        let lib = store.default_library;
        for i in 0..common {
            let mut d = ItemDraft::new("journalArticle").with_field("title", format!("C{i}"));
            d.tags = vec![ItemTag::manual("common")];
            store.items.create(lib, d).await.unwrap();
        }
        for i in 0..rare {
            let mut d = ItemDraft::new("journalArticle").with_field("title", format!("R{i}"));
            d.tags = vec![ItemTag::manual("rare")];
            store.items.create(lib, d).await.unwrap();
        }
        store
    }

    fn probed(store: &crate::Store, tag: &str, window: i64, rows: i64) -> bool {
        let filter = ItemFilter {
            library_id: store.default_library,
            tags: vec![tag.to_string()],
            ..Default::default()
        };
        let conn = store.db().conn().unwrap();
        crate::filter::should_walk_probed(&conn, &filter, window, rows)
    }

    #[tokio::test]
    async fn it_answers_the_same_question_as_the_exact_rule() {
        let store = library(60, 3).await;
        // Crossover for these numbers is sqrt(400 * 4) = 40.
        let (window, rows) = (4, 400);
        assert_eq!(crate::filter::crossover(rows, window), Some(40));

        assert!(probed(&store, "common", window, rows), "60 is past the crossover");
        assert!(!probed(&store, "rare", window, rows), "3 is not");
        // And the exact rule agrees, which is the point: the probe is a cheaper
        // way to ask, not a different question.
        assert!(crate::filter::should_walk(60, window, rows));
        assert!(!crate::filter::should_walk(3, window, rows));
    }

    #[tokio::test]
    async fn a_tag_nobody_uses_is_not_walked() {
        let store = library(60, 3).await;
        assert!(!probed(&store, "no-such-tag", 4, 400));
    }

    #[tokio::test]
    async fn only_a_single_positive_tag_is_probed() {
        // Two ANDed tags make the result no larger than either set, so probing
        // one would be an upper bound — and an upper bound can only choose the
        // walk wrongly.
        let store = library(60, 3).await;
        let conn = store.db().conn().unwrap();
        let lib = store.default_library;
        let with = |tags: Vec<String>| {
            let filter = ItemFilter { library_id: lib, tags, ..Default::default() };
            crate::filter::should_walk_probed(&conn, &filter, 4, 400)
        };
        assert!(with(vec!["common".into()]));
        assert!(!with(vec!["common".into(), "rare".into()]), "two tags");
        assert!(!with(vec!["-common".into()]), "negated");
        assert!(!with(vec![]), "none");
    }

    #[tokio::test]
    async fn without_statistics_it_declines() {
        let store = library(60, 3).await;
        assert!(!probed(&store, "common", 4, 0), "no row estimate, no decision");
        assert!(!probed(&store, "common", 0, 400), "no page, no decision");
    }
}
