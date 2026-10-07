use rusqlite::{params, params_from_iter};
use serde_json::Value;
use yk_core::{Key, Result};

use crate::{sql_err, Db};

impl Db {
    pub async fn pending_file_cleanup(&self, limit: u32) -> Result<Vec<(i64, Key)>> {
        self.call(move |c| {
            let pending: bool = c
                .query_row("SELECT EXISTS(SELECT 1 FROM file_cleanup)", [], |r| {
                    r.get(0)
                })
                .map_err(sql_err)?;
            if !pending {
                return Ok(Vec::new());
            }
            // Restoring an item with the same key makes its files live again.
            c.execute(
                "DELETE FROM file_cleanup WHERE item_key IN (SELECT key FROM items)",
                [],
            )
            .map_err(sql_err)?;
            let mut stmt = c.prepare(
                "SELECT library_id,item_key FROM file_cleanup ORDER BY library_id,item_key LIMIT ?1"
            ).map_err(sql_err)?;
            let rows = stmt
                .query_map([limit], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(sql_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(sql_err)?;
            rows.into_iter()
                .map(|(lib, key)| Ok((lib, Key::parse(&key)?)))
                .collect()
        })
        .await
    }

    pub async fn finish_file_cleanup(&self, keys: Vec<(i64, Key)>) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        self.call(move |c| {
            let tx = crate::write_tx(c)?;
            for (lib, key) in keys {
                tx.execute(
                    "DELETE FROM file_cleanup WHERE library_id=?1 AND item_key=?2",
                    params![lib, key.as_str()],
                )
                .map_err(sql_err)?;
            }
            tx.commit().map_err(sql_err)
        })
        .await
    }

    pub async fn task_records(&self) -> Result<Vec<Value>> {
        self.call(|c| {
            let mut stmt = c
                .prepare("SELECT state FROM task_records")
                .map_err(sql_err)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(sql_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(sql_err)?;
            rows.into_iter()
                .map(|s| serde_json::from_str(&s).map_err(Into::into))
                .collect()
        })
        .await
    }

    pub async fn save_task_record(&self, id: String, state: Value) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO task_records(id,state) VALUES (?1,?2)
                 ON CONFLICT(id) DO UPDATE SET state=excluded.state",
                params![id, state.to_string()],
            )
            .map_err(sql_err)?;
            Ok(())
        })
        .await
    }

    pub async fn forget_task_records(&self, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.call(move |c| {
            for chunk in crate::filter::chunks(&ids) {
                c.execute(
                    &format!(
                        "DELETE FROM task_records WHERE id IN ({})",
                        crate::filter::placeholders(chunk.len())
                    ),
                    params_from_iter(chunk),
                )
                .map_err(sql_err)?;
            }
            Ok(())
        })
        .await
    }
}
