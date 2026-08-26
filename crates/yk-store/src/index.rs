//! Maintenance of the derived search structures that live alongside `items`.
//!
//! Centralised here so every write path (create / update / delete / import)
//! keeps the FTS, trigram and embedding queues consistent — there is exactly
//! one place that knows how a row becomes searchable.

use rusqlite::{params, Connection};
use yk_core::model::Item;
use yk_core::{text, Result};

use crate::db::sql_err;

/// Maximum characters fed to the embedding provider per item.
const EMBED_BUDGET: usize = 1200;

/// Join tokens so the `unicode61` tokenizer can index CJK, which it otherwise
/// treats as one long word.
fn tokenized(input: &str) -> String {
    text::tokenize(input).join(" ")
}

fn creators_text(item: &Item) -> String {
    item.creators
        .iter()
        .map(|c| c.display())
        .collect::<Vec<_>>()
        .join(" ")
}

fn tags_text(item: &Item) -> String {
    item.tags.iter().map(|t| t.tag.as_str()).collect::<Vec<_>>().join(" ")
}

/// Text handed to the embedding provider: the parts that actually carry meaning.
pub fn embed_text(item: &Item) -> String {
    let mut s = String::with_capacity(EMBED_BUDGET);
    s.push_str(item.title());
    s.push_str(". ");
    s.push_str(&creators_text(item));
    s.push_str(". ");
    if let Some(v) = item.field("publicationTitle").or_else(|| item.field("bookTitle")) {
        s.push_str(v);
        s.push_str(". ");
    }
    if let Some(a) = item.field("abstractNote") {
        s.push_str(a);
    }
    let tags = tags_text(item);
    if !tags.is_empty() {
        s.push_str(" [");
        s.push_str(&tags);
        s.push(']');
    }
    truncate_chars(&s, EMBED_BUDGET)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// Stable, cheap content hash used to skip re-embedding unchanged text.
pub fn content_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

/// Rebuild every derived row for a single item. Idempotent.
pub fn reindex(tx: &Connection, id: i64, item: &Item) -> Result<()> {
    remove(tx, &[id])?;

    // Trashed items stay in the database but must not surface in search.
    if item.deleted {
        return Ok(());
    }

    let title = item.title();
    let creators = creators_text(item);
    let tags = tags_text(item);
    let body = item.search_text();

    tx.execute(
        "INSERT INTO items_fts(rowid, title, creators, body, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, tokenized(title), tokenized(&creators), tokenized(&body), tokenized(&tags)],
    )
    .map_err(sql_err)?;

    let trgm = text::normalize(&format!("{title} {creators} {tags}"));
    tx.execute("INSERT INTO items_trgm(rowid, text) VALUES (?1, ?2)", params![id, trgm])
        .map_err(sql_err)?;

    // Only meaningful items are worth embedding.
    if item.is_regular() {
        let text = embed_text(item);
        let hash = content_hash(&text);
        let unchanged: bool = tx
            .query_row(
                "SELECT 1 FROM item_vectors WHERE item_id = ?1 AND content_hash = ?2",
                params![id, hash],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !unchanged {
            tx.execute(
                "INSERT INTO embed_queue(item_id, library_id, text, content_hash, queued_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(item_id) DO UPDATE SET
                    text = excluded.text,
                    content_hash = excluded.content_hash,
                    queued_at = excluded.queued_at",
                params![id, item.library_id, text, hash, yk_core::now_ms()],
            )
            .map_err(sql_err)?;
        }
    }
    Ok(())
}

/// Drop all derived rows for the given item ids.
pub fn remove(tx: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        tx.execute("DELETE FROM items_fts WHERE rowid = ?1", params![id]).map_err(sql_err)?;
        tx.execute("DELETE FROM items_trgm WHERE rowid = ?1", params![id]).map_err(sql_err)?;
        tx.execute("DELETE FROM embed_queue WHERE item_id = ?1", params![id]).map_err(sql_err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::{Creator, ItemDraft};
    use yk_core::Key;

    fn sample() -> Item {
        ItemDraft::new("journalArticle")
            .with_field("title", "扩散模型综述 A Survey of Diffusion Models")
            .with_field("abstractNote", "We review diffusion.")
            .with_creator(Creator { last_name: Some("Zhang".into()), ..Default::default() })
            .into_item(Key::generate(), 1, 1)
    }

    #[test]
    fn embed_text_is_bounded_and_informative() {
        let t = embed_text(&sample());
        assert!(t.contains("Diffusion"));
        assert!(t.contains("Zhang"));
        assert!(t.chars().count() <= EMBED_BUDGET);
    }

    #[test]
    fn content_hash_is_stable_and_discriminating() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
    }

    #[test]
    fn tokenized_splits_cjk() {
        let t = tokenized("扩散模型");
        assert!(t.contains("扩散"));
        assert!(t.split(' ').count() > 1);
    }
}
