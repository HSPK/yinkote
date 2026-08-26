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
                clauses.push(format!(
                    "EXISTS (SELECT 1 FROM collection_items ci WHERE ci.item_id = i.id \
                     AND ci.collection_id IN ({}))",
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
            clauses.push(format!(
                "{} (SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id \
                 WHERE it.item_id = i.id AND t.name = ?)",
                if negated { "NOT EXISTS" } else { "EXISTS" }
            ));
            params.push(SqlValue::Text(name.to_string()));
        }

        Predicate { sql: clauses.join(" AND "), params }
    }
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
    fn negated_tags_use_not_exists() {
        let f = ItemFilter {
            library_id: 1,
            tags: vec!["llm".into(), "-obsolete".into()],
            ..Default::default()
        };
        let p = Predicate::build(&f, None);
        assert!(p.sql.contains("EXISTS"));
        assert!(p.sql.contains("NOT EXISTS"));
        assert_eq!(p.params.len(), 3);
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
