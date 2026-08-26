//! Smart collections: saved queries that behave like collections.

use async_trait::async_trait;
use rusqlite::{params, OptionalExtension};
use yk_core::model::*;
use yk_core::ports::SmartCollectionRepository;
use yk_core::{Error, Key, Result};

use crate::db::{sql_err, write_tx, Db};

const SELECT: &str = "SELECT key, library_id, name, query, mode, sort, direction, sort_index, \
                      color, icon, version FROM smart_collections";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<SmartCollection> {
    Ok(SmartCollection {
        key: Key::parse(&r.get::<_, String>(0)?).unwrap_or_else(|_| Key::generate()),
        library_id: r.get(1)?,
        name: r.get(2)?,
        query: r.get(3)?,
        mode: r.get(4)?,
        sort: r.get(5)?,
        direction: r.get(6)?,
        sort_index: r.get(7)?,
        color: r.get(8)?,
        icon: r.get(9)?,
        version: r.get(10)?,
        item_count: None,
    })
}

fn bump(tx: &rusqlite::Connection, library_id: i64) -> Result<i64> {
    tx.execute("UPDATE libraries SET version = version + 1 WHERE id = ?1", params![library_id])
        .map_err(sql_err)?;
    tx.query_row("SELECT version FROM libraries WHERE id=?1", params![library_id], |r| r.get(0))
        .map_err(sql_err)
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("smart collection name must not be empty"));
    }
    Ok(name.to_string())
}

#[derive(Clone)]
pub struct SqliteSmartCollectionRepository {
    db: Db,
}

impl SqliteSmartCollectionRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SmartCollectionRepository for SqliteSmartCollectionRepository {
    async fn list(&self, library_id: i64) -> Result<Vec<SmartCollection>> {
        self.db
            .call(move |c| {
                let sql = format!("{SELECT} WHERE library_id=?1 ORDER BY sort_index, name");
                let mut stmt = c.prepare_cached(&sql).map_err(sql_err)?;
                let out = stmt
                    .query_map(params![library_id], map)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_err);
                out
            })
            .await
    }

    async fn get(&self, library_id: i64, key: &Key) -> Result<SmartCollection> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let sql = format!("{SELECT} WHERE library_id=?1 AND key=?2");
                c.prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_row(params![library_id, key.as_str()], map)
                    .optional()
                    .map_err(sql_err)?
                    .ok_or_else(|| Error::not_found(format!("smart collection {key}")))
            })
            .await
    }

    async fn create(
        &self,
        library_id: i64,
        draft: SmartCollectionDraft,
    ) -> Result<SmartCollection> {
        self.db
            .call(move |c| {
                let name = clean_name(&draft.name)?;
                let key = draft.key.clone().unwrap_or_else(Key::generate);
                let tx = write_tx(c)?;
                let version = bump(&tx, library_id)?;
                let sort_index: f64 = tx
                    .query_row(
                        "SELECT COALESCE(MAX(sort_index), 0) + 1 FROM smart_collections \
                         WHERE library_id=?1",
                        params![library_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(0.0);

                tx.execute(
                    "INSERT INTO smart_collections
                       (library_id, key, name, query, mode, sort, direction, sort_index,
                        color, icon, version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![
                        library_id,
                        key.as_str(),
                        name,
                        draft.query.trim(),
                        draft.mode.as_deref().unwrap_or("hybrid"),
                        draft.sort.as_deref().unwrap_or("dateModified"),
                        draft.direction.as_deref().unwrap_or("desc"),
                        sort_index,
                        draft.color.as_deref(),
                        draft.icon.as_deref(),
                        version
                    ],
                )
                .map_err(sql_err)?;

                let sql = format!("{SELECT} WHERE library_id=?1 AND key=?2");
                let out = tx
                    .query_row(&sql, params![library_id, key.as_str()], map)
                    .map_err(sql_err)?;
                tx.commit().map_err(sql_err)?;
                Ok(out)
            })
            .await
    }

    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: SmartCollectionPatch,
    ) -> Result<SmartCollection> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let exists: Option<i64> = tx
                    .query_row(
                        "SELECT id FROM smart_collections WHERE library_id=?1 AND key=?2",
                        params![library_id, key.as_str()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql_err)?;
                let id = exists
                    .ok_or_else(|| Error::not_found(format!("smart collection {key}")))?;

                // One statement per present field keeps the SQL trivial and the
                // "leave untouched" semantics obvious.
                if let Some(name) = &patch.name {
                    let name = clean_name(name)?;
                    tx.execute(
                        "UPDATE smart_collections SET name=?1 WHERE id=?2",
                        params![name, id],
                    )
                    .map_err(sql_err)?;
                }
                for (column, value) in [
                    ("query", patch.query.as_deref()),
                    ("mode", patch.mode.as_deref()),
                    ("sort", patch.sort.as_deref()),
                    ("direction", patch.direction.as_deref()),
                ] {
                    if let Some(v) = value {
                        tx.execute(
                            &format!("UPDATE smart_collections SET {column}=?1 WHERE id=?2"),
                            params![v.trim(), id],
                        )
                        .map_err(sql_err)?;
                    }
                }
                if let Some(si) = patch.sort_index {
                    tx.execute(
                        "UPDATE smart_collections SET sort_index=?1 WHERE id=?2",
                        params![si, id],
                    )
                    .map_err(sql_err)?;
                }
                // Nullable appearance: `Some(None)` clears, absent leaves alone.
                for (column, value) in
                    [("color", patch.color.as_ref()), ("icon", patch.icon.as_ref())]
                {
                    if let Some(v) = value {
                        tx.execute(
                            &format!("UPDATE smart_collections SET {column}=?1 WHERE id=?2"),
                            params![v.as_deref(), id],
                        )
                        .map_err(sql_err)?;
                    }
                }

                let version = bump(&tx, library_id)?;
                tx.execute(
                    "UPDATE smart_collections SET version=?1 WHERE id=?2",
                    params![version, id],
                )
                .map_err(sql_err)?;

                let sql = format!("{SELECT} WHERE id=?1");
                let out = tx.query_row(&sql, params![id], map).map_err(sql_err)?;
                tx.commit().map_err(sql_err)?;
                Ok(out)
            })
            .await
    }

    async fn delete(&self, library_id: i64, key: &Key) -> Result<u64> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let n = tx
                    .execute(
                        "DELETE FROM smart_collections WHERE library_id=?1 AND key=?2",
                        params![library_id, key.as_str()],
                    )
                    .map_err(sql_err)? as u64;
                bump(&tx, library_id)?;
                tx.commit().map_err(sql_err)?;
                Ok(n)
            })
            .await
    }
}
