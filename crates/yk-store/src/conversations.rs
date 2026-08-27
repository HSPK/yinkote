//! Conversation and message persistence.
//!
//! Deliberately dumb storage: appending a message is the only write an agent
//! loop needs, and keeping the repository free of any reasoning means the loop
//! can be replaced without touching history.

use async_trait::async_trait;
use rusqlite::{params, OptionalExtension};
use yk_core::model::*;
use yk_core::ports::ConversationRepository;
use yk_core::{now_ms, Error, Key, Result};

use crate::db::{sql_err, write_tx, Db};

const SELECT: &str = "SELECT c.key, c.library_id, c.title, c.scope, \
     (SELECT count(*) FROM messages m WHERE m.conversation_id = c.id), \
     c.created_at, c.updated_at FROM conversations c";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        key: Key::parse(&r.get::<_, String>(0)?).unwrap_or_else(|_| Key::generate()),
        library_id: r.get(1)?,
        title: r.get(2)?,
        scope: r.get(3)?,
        message_count: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

fn map_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let meta: Option<String> = r.get(3)?;
    Ok(Message {
        id: r.get(0)?,
        role: r.get(1)?,
        content: r.get(2)?,
        meta: meta.and_then(|m| serde_json::from_str(&m).ok()),
        mentions: Vec::new(),
        created_at: r.get(4)?,
    })
}

fn id_of(conn: &rusqlite::Connection, library_id: i64, key: &Key) -> Result<i64> {
    conn.query_row(
        "SELECT id FROM conversations WHERE library_id=?1 AND key=?2",
        params![library_id, key.as_str()],
        |r| r.get(0),
    )
    .optional()
    .map_err(sql_err)?
    .ok_or_else(|| Error::not_found(format!("conversation {key}")))
}

const ROLES: [&str; 4] = ["user", "assistant", "tool", "system"];

#[derive(Clone)]
pub struct SqliteConversationRepository {
    db: Db,
}

impl SqliteConversationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ConversationRepository for SqliteConversationRepository {
    async fn list(&self, library_id: i64, limit: u32) -> Result<Vec<Conversation>> {
        let limit = limit.clamp(1, 500);
        self.db
            .call(move |c| {
                let sql =
                    format!("{SELECT} WHERE c.library_id=?1 ORDER BY c.updated_at DESC LIMIT ?2");
                let mut stmt = c.prepare_cached(&sql).map_err(sql_err)?;
                let out = stmt
                    .query_map(params![library_id, limit], map)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_err);
                out
            })
            .await
    }

    async fn get(&self, library_id: i64, key: &Key) -> Result<Conversation> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let sql = format!("{SELECT} WHERE c.library_id=?1 AND c.key=?2");
                c.prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_row(params![library_id, key.as_str()], map)
                    .optional()
                    .map_err(sql_err)?
                    .ok_or_else(|| Error::not_found(format!("conversation {key}")))
            })
            .await
    }

    async fn create(
        &self,
        library_id: i64,
        title: &str,
        scope: Option<&str>,
    ) -> Result<Conversation> {
        let title = title.trim().to_string();
        let scope = scope.map(str::to_string);
        self.db
            .call(move |c| {
                let key = Key::generate();
                let now = now_ms();
                c.execute(
                    "INSERT INTO conversations(library_id, key, title, scope, created_at, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?5)",
                    params![library_id, key.as_str(), title, scope, now],
                )
                .map_err(sql_err)?;
                Ok(Conversation {
                    key,
                    library_id,
                    title,
                    scope,
                    message_count: 0,
                    created_at: now,
                    updated_at: now,
                })
            })
            .await
    }

    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: ConversationPatch,
    ) -> Result<Conversation> {
        let key = key.clone();
        let title = match &patch.title {
            Some(t) if t.trim().is_empty() => {
                return Err(Error::invalid("conversation title must not be empty"))
            }
            Some(t) => Some(t.trim().to_string()),
            None => None,
        };
        // An empty scope string means the same thing as no scope; storing it
        // would give the same state two representations.
        let scope = patch.scope.map(|s| s.filter(|v| !v.trim().is_empty()));

        self.db
            .call(move |c| {
                // Built from what was actually asked for, so a patch that only
                // sets the scope cannot quietly rewrite the title.
                let mut sets: Vec<&str> = Vec::new();
                let mut args: Vec<rusqlite::types::Value> = Vec::new();
                if let Some(title) = &title {
                    sets.push("title=?");
                    args.push(title.clone().into());
                }
                if let Some(scope) = &scope {
                    sets.push("scope=?");
                    args.push(match scope {
                        Some(v) => v.clone().into(),
                        None => rusqlite::types::Value::Null,
                    });
                }
                if !sets.is_empty() {
                    sets.push("updated_at=?");
                    args.push(now_ms().into());
                    args.push(library_id.into());
                    args.push(key.to_string().into());
                    let n = c
                        .execute(
                            &format!(
                                "UPDATE conversations SET {} WHERE library_id=? AND key=?",
                                sets.join(", ")
                            ),
                            rusqlite::params_from_iter(args),
                        )
                        .map_err(sql_err)?;
                    if n == 0 {
                        return Err(Error::not_found(format!("conversation {key}")));
                    }
                }
                let sql = format!("{SELECT} WHERE c.library_id=?1 AND c.key=?2");
                c.query_row(&sql, params![library_id, key.as_str()], map).map_err(sql_err)
            })
            .await
    }

    async fn delete(&self, library_id: i64, key: &Key) -> Result<u64> {
        let key = key.clone();
        self.db
            .call(move |c| {
                // Messages cascade; the index makes that cheap.
                Ok(c.execute(
                    "DELETE FROM conversations WHERE library_id=?1 AND key=?2",
                    params![library_id, key.as_str()],
                )
                .map_err(sql_err)? as u64)
            })
            .await
    }

    async fn messages(&self, library_id: i64, key: &Key) -> Result<Vec<Message>> {
        let key = key.clone();
        self.db
            .call(move |c| {
                let id = id_of(c, library_id, &key)?;
                let mut stmt = c
                    .prepare_cached(
                        "SELECT id, role, content, meta, created_at FROM messages \
                         WHERE conversation_id=?1 ORDER BY id",
                    )
                    .map_err(sql_err)?;
                let mut out = stmt
                    .query_map(params![id], map_message)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_err)?;

                // One query for the whole thread rather than one per message:
                // a conversation is read on every keystroke of a live turn.
                let mut mentions = c
                    .prepare_cached(
                        "SELECT mm.message_id, mm.item_key FROM message_mentions mm \
                         JOIN messages m ON m.id = mm.message_id \
                         WHERE m.conversation_id = ?1",
                    )
                    .map_err(sql_err)?;
                let mut rows = mentions.query(params![id]).map_err(sql_err)?;
                while let Some(row) = rows.next().map_err(sql_err)? {
                    let message_id: i64 = row.get(0).map_err(sql_err)?;
                    let raw: String = row.get(1).map_err(sql_err)?;
                    if let (Some(message), Ok(item)) =
                        (out.iter_mut().find(|m| m.id == message_id), Key::parse(&raw))
                    {
                        message.mentions.push(item);
                    }
                }
                Ok(out)
            })
            .await
    }

    async fn append(&self, library_id: i64, key: &Key, draft: MessageDraft) -> Result<Message> {
        let key = key.clone();
        if !ROLES.contains(&draft.role.as_str()) {
            return Err(Error::invalid(format!("unknown message role '{}'", draft.role)));
        }
        self.db
            .call(move |c| {
                let tx = write_tx(c)?;
                let id = id_of(&tx, library_id, &key)?;
                let now = now_ms();
                let meta = draft.meta.as_ref().map(|m| m.to_string());
                tx.execute(
                    "INSERT INTO messages(conversation_id, role, content, meta, created_at)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![id, draft.role, draft.content, meta, now],
                )
                .map_err(sql_err)?;
                let message_id = tx.last_insert_rowid();

                // Written with the message, not after it. A mention index that
                // is maintained separately can disagree with the conversation
                // it describes, and the disagreement is invisible until
                // somebody notices a paper missing from its own history.
                for key in &draft.mentions {
                    tx.execute(
                        "INSERT INTO message_mentions(message_id, item_key) VALUES (?1,?2)
                         ON CONFLICT DO NOTHING",
                        params![message_id, key.as_str()],
                    )
                    .map_err(sql_err)?;
                }

                // Recency ordering in the sidebar depends on this.
                tx.execute(
                    "UPDATE conversations SET updated_at=?1 WHERE id=?2",
                    params![now, id],
                )
                .map_err(sql_err)?;
                tx.commit().map_err(sql_err)?;

                Ok(Message {
                    id: message_id,
                    role: draft.role,
                    content: draft.content,
                    meta: draft.meta,
                    mentions: draft.mentions,
                    created_at: now,
                })
            })
            .await
    }

    async fn mentioning(&self, library_id: i64, item: &Key) -> Result<Vec<Conversation>> {
        let item = item.to_string();
        self.db
            .call(move |c| {
                // EXISTS rather than a join: a conversation that mentions the
                // same paper in nine messages is still one conversation, and
                // de-duplicating afterwards is work that need not happen.
                let sql = format!(
                    "{SELECT} WHERE c.library_id = ?1 AND EXISTS (                        SELECT 1 FROM messages m                        JOIN message_mentions mm ON mm.message_id = m.id                        WHERE m.conversation_id = c.id AND mm.item_key = ?2)                      ORDER BY c.updated_at DESC LIMIT 50"
                );
                let rows = c
                    .prepare_cached(&sql)
                    .map_err(sql_err)?
                    .query_map(params![library_id, item], map)
                    .map_err(sql_err)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_err)?;
                Ok(rows)
            })
            .await
    }
}
