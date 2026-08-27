//! Translation of a domain [`ItemFilter`] into an indexed SQL predicate.
//!
//! Kept separate from the repository so it can be unit-tested and reused by
//! listing, counting and faceting without duplication.

use rusqlite::types::Value as SqlValue;
use yk_core::query::{Direction, ItemFilter, SortField, TrashScope};

#[derive(Default)]
pub struct Predicate {
    pub sql: String,
    pub params: Vec<SqlValue>,
}

impl Predicate {
    /// `collection_ids` must already be resolved from keys by the caller,
    /// including descendants when the filter is recursive.
    pub fn build(filter: &ItemFilter, collection_ids: Option<&[i64]>) -> Self {
        let mut clauses: Vec<String> = Vec::with_capacity(8);
        let mut params: Vec<SqlValue> = Vec::with_capacity(8);

        clauses.push("i.library_id = ?".into());
        params.push(SqlValue::Integer(filter.library_id));

        match filter.trash {
            TrashScope::Exclude => clauses.push("i.deleted = 0".into()),
            TrashScope::Only => clauses.push("i.deleted = 1".into()),
            TrashScope::Include => {}
        }

        if filter.top_level_only {
            clauses.push("i.parent_id IS NULL".into());
        }

        if let Some(since) = filter.since {
            clauses.push("i.version > ?".into());
            params.push(SqlValue::Integer(since));
        }

        if !filter.item_types.is_empty() {
            clauses.push(format!("i.item_type IN ({})", placeholders(filter.item_types.len())));
            params.extend(filter.item_types.iter().map(|t| SqlValue::Text(t.clone())));
        }

        // A predicate is one statement, so these two clauses cannot be split
        // the way a plain `IN` list can — see `chunks`. Both are bounded
        // today (a page is at most MAX_LIMIT keys; a collection subtree is
        // far short of SQLite's own limit), but a caller that grows past that
        // has to page rather than widen the list.
        if let Some(keys) = &filter.keys {
            if keys.is_empty() {
                clauses.push("0".into());
            } else {
                clauses.push(format!("i.key IN ({})", placeholders(keys.len())));
                params.extend(keys.iter().map(|k| SqlValue::Text(k.to_string())));
            }
        }

        if let Some(ids) = collection_ids {
            if ids.is_empty() {
                clauses.push("0".into());
            } else {
                // `IN` over the membership list, not a correlated `EXISTS`.
                //
                // `EXISTS` reads as the natural phrasing — "items that are in
                // this collection" — and makes SQLite drive from `items`,
                // walking the whole library and probing memberships for each
                // row. `IN` drives the other way: it materialises the
                // collection's members once and probes `items`. Identical
                // results, 41.8ms against 2.0ms on a hundred-thousand-item
                // library. Same shape as the tag clause below, and the same
                // reason `graph::TAG_SQL` spells out `CROSS JOIN`.
                clauses.push(format!(
                    "i.id IN (SELECT ci.item_id FROM collection_items ci \
                     WHERE ci.collection_id IN ({}))",
                    placeholders(ids.len())
                ));
                params.extend(ids.iter().map(|id| SqlValue::Integer(*id)));
            }
        }

        for tag in &filter.tags {
            let (negated, name) = match tag.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, tag.as_str()),
            };
            // `IN` over an id list beats a correlated `EXISTS`: SQLite can
            // materialise the (small) set of tagged items once from
            // `idx_item_tags_tag` instead of probing per candidate row.
            // Measured 3.4x faster on a 100k-item library.
            //
            // The inner scalar subquery yields NULL for an unknown tag, so the
            // list is empty — `IN` is false and `NOT IN` is true, both correct,
            // and no NULL can enter the list because `item_id` is NOT NULL.
            clauses.push(format!(
                "i.id {} (SELECT it.item_id FROM item_tags it WHERE it.tag_id = \
                 (SELECT id FROM tags WHERE library_id = ? AND name = ?))",
                if negated { "NOT IN" } else { "IN" }
            ));
            params.push(SqlValue::Integer(filter.library_id));
            params.push(SqlValue::Text(name.to_string()));
        }

        Predicate { sql: clauses.join(" AND "), params }
    }
}

/// The most values one statement may bind.
///
/// SQLite's own limit is 32766 in current builds and 999 in older ones. This
/// sits below both, because the cost of another round of a prepared statement
/// is microseconds and the cost of guessing wrong is an error on an operation
/// that has always worked — met by whoever's library happens to be the first
/// one big enough. "Select all and move to trash" on forty thousand items
/// reproduced it exactly.
pub const MAX_BOUND: usize = 900;

/// Break a list into runs small enough to bind in one statement.
///
/// Chunking happens *inside* the caller's transaction, so an operation over
/// forty thousand keys is still all-or-nothing — the ceiling goes away without
/// atomicity going with it.
pub fn chunks<T>(values: &[T]) -> impl Iterator<Item = &[T]> {
    values.chunks(MAX_BOUND.max(1))
}

pub fn placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}

/// Whitelisted ORDER BY clause — never interpolates user input.
pub fn order_by(sort: SortField, dir: Direction) -> String {
    let col = match sort {
        SortField::DateModified => "i.date_modified",
        SortField::DateAdded => "i.date_added",
        SortField::Title => "i.sort_title",
        SortField::Creator => "i.sort_creator",
        SortField::Year => "i.year",
        SortField::ItemType => "i.item_type",
        // Denormalised and kept up to date by trigger, like every other
        // sortable value here: worked out on demand it was 109ms against 9ms,
        // because a correlated subquery in ORDER BY costs the whole library on
        // every page. See `012_attachment_rank.sql`.
        SortField::Attachment => "i.attachment_rank",
        // Relevance is resolved by the search layer; fall back to recency.
        SortField::Relevance => "i.date_modified",
    };
    // `id` breaks ties so keyset pagination is stable.
    format!("ORDER BY {col} {0}, i.id {0}", dir.sql())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::Key;

    #[test]
    fn base_filter_is_indexed() {
        let f = ItemFilter { library_id: 1, ..Default::default() };
        let p = Predicate::build(&f, None);
        assert_eq!(p.sql, "i.library_id = ? AND i.deleted = 0");
        assert_eq!(p.params.len(), 1);
    }

    #[test]
    fn negated_tags_use_not_in() {
        let f = ItemFilter {
            library_id: 1,
            tags: vec!["llm".into(), "-obsolete".into()],
            ..Default::default()
        };
        let p = Predicate::build(&f, None);
        assert!(p.sql.contains("i.id IN "));
        assert!(p.sql.contains("i.id NOT IN "));
        // library_id, then (library_id, name) per tag.
        assert_eq!(p.params.len(), 5);
    }

    #[test]
    fn empty_key_set_matches_nothing() {
        let f = ItemFilter { library_id: 1, keys: Some(vec![]), ..Default::default() };
        assert!(Predicate::build(&f, None).sql.contains(" 0"));
    }

    #[test]
    fn key_filter_binds_all_keys() {
        let f = ItemFilter {
            library_id: 1,
            keys: Some(vec![Key::generate(), Key::generate()]),
            ..Default::default()
        };
        assert_eq!(Predicate::build(&f, None).params.len(), 3);
    }

    #[test]
    fn order_by_is_whitelisted() {
        let s = order_by(SortField::Title, Direction::Asc);
        assert_eq!(s, "ORDER BY i.sort_title ASC, i.id ASC");
    }
}
