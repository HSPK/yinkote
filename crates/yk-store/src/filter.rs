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
    /// Whether this is nothing but "a live item in this library".
    ///
    /// A plain browse can be driven straight down the index that already holds
    /// the sort order; anything narrower — a tag, a collection — is better off
    /// letting the planner choose, since it may be far more selective than the
    /// ordering. See [`sort_index`].
    pub base_only: bool,
    /// Whether this is the base plus positive tag filters and nothing else.
    ///
    /// The one shape where [`TagForm::Probe`] is available, because it is the
    /// one shape whose only narrowing clause is the tag. See [`should_walk`].
    pub tags_only: bool,
}

/// How a tag filter is written, which decides the plan SQLite picks.
///
/// The two forms return identical rows and differ by two orders of magnitude
/// in either direction depending on how common the tag is. See [`should_walk`]
/// for which to use; measurements are in `docs/16` 3.117.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TagForm {
    /// `i.id IN (SELECT ...)`: build the set of tagged items once, then probe
    /// `items`. The planner then has to sort that set to honour ORDER BY.
    Materialise,
    /// `EXISTS (SELECT ... WHERE it.item_id = i.id)`: walk `items` in sort
    /// order and ask of each row whether it carries the tag. No sort at all,
    /// but it reads until the page is full.
    Probe,
}

impl Predicate {
    /// `collection_ids` must already be resolved from keys by the caller,
    /// including descendants when the filter is recursive.
    pub fn build(filter: &ItemFilter, collection_ids: Option<&[i64]>) -> Self {
        Self::build_with(filter, collection_ids, TagForm::Materialise)
    }

    pub fn build_with(
        filter: &ItemFilter,
        collection_ids: Option<&[i64]>,
        form: TagForm,
    ) -> Self {
        let mut clauses: Vec<String> = Vec::with_capacity(8);
        let mut params: Vec<SqlValue> = Vec::with_capacity(8);

        clauses.push("i.library_id = ?".into());
        params.push(SqlValue::Integer(filter.library_id));
        let base_clauses = 2; // library and trash scope

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

        let mut positive_tags = 0;
        for tag in &filter.tags {
            let (negated, name) = match tag.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, tag.as_str()),
            };
            if !negated {
                positive_tags += 1;
            }

            // The inner scalar subquery yields NULL for an unknown tag, so the
            // set is empty — `IN` is false and `NOT IN` is true, both correct,
            // and no NULL can enter the list because `item_id` is NOT NULL.
            //
            // A negated tag is always materialised. `NOT EXISTS` would have to
            // be asked of every row in the library, which is the shape the
            // probe form exists to avoid.
            let use_probe = form == TagForm::Probe && !negated;
            clauses.push(if use_probe {
                "EXISTS (SELECT 1 FROM item_tags it WHERE it.item_id = i.id AND it.tag_id = \
                 (SELECT id FROM tags WHERE library_id = ? AND name = ?))"
                    .to_string()
            } else {
                format!(
                    "i.id {} (SELECT it.item_id FROM item_tags it WHERE it.tag_id = \
                     (SELECT id FROM tags WHERE library_id = ? AND name = ?))",
                    if negated { "NOT IN" } else { "IN" }
                )
            });
            params.push(SqlValue::Integer(filter.library_id));
            params.push(SqlValue::Text(name.to_string()));
        }

        Predicate {
            base_only: clauses.len() <= base_clauses,
            tags_only: positive_tags > 0
                && clauses.len() == base_clauses + positive_tags
                && filter.tags.len() == positive_tags,
            sql: clauses.join(" AND "),
            params,
        }
    }
}

/// Whether to walk the sort order and probe, rather than build the tagged set
/// and sort it.
///
/// Both plans are correct; the cost is the whole story.
///
/// * Materialise costs roughly `total` — it collects that many ids and sorts
///   them to honour ORDER BY, however few the page needs.
/// * Probe costs roughly `rows × window / total` — it walks the sort order and
///   keeps `window` of every `total/rows` it passes.
///
/// Setting the two equal gives the crossover: probe wins once
/// `total > sqrt(rows × window)`. Measured on a 131k-item library at a page of
/// 100 (crossover ≈ 3600): a tag on 4 items takes 0.1ms materialised and 50ms
/// probed; a tag on 28,709 takes 33ms materialised and 0.2ms probed.
///
/// `rows` is an estimate — `sqlite_stat1` is exactly the right precision for a
/// decision whose two sides differ by orders of magnitude either way.
pub fn should_walk(total: i64, window: i64, rows: i64) -> bool {
    if total <= 0 || window <= 0 || rows <= 0 {
        return false;
    }
    let crossover = ((rows as f64) * (window as f64)).sqrt();
    (total as f64) > crossover
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

/// The index that already holds a given order.
///
/// Named explicitly, because leaving it to the planner is how a plain browse
/// went from 9ms to 69ms: adding `idx_items_attachment` gave SQLite a second
/// index with the same `(library_id, deleted)` prefix, it chose that one, and
/// then sorted a hundred and thirty thousand rows to get the order it had
/// thrown away. Nothing about the *results* changed, which is why only a plan
/// assertion can see it — and the assertion has to run against a database with
/// statistics, or the planner it questions is not the one in production.
///
/// Every one of these indexes leads with `(library_id, deleted)`, so the hint
/// is valid for the base filter and for nothing else. See `Predicate::base_only`.
pub fn sort_index(sort: SortField) -> &'static str {
    match sort {
        SortField::DateModified | SortField::Relevance => "idx_items_modified",
        SortField::DateAdded => "idx_items_added",
        SortField::Title => "idx_items_title",
        SortField::Creator => "idx_items_creator",
        SortField::Year => "idx_items_year",
        SortField::ItemType => "idx_items_type",
        SortField::Attachment => "idx_items_attachment",
    }
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

#[cfg(test)]
mod tag_plan_tests {
    use super::*;

    fn filter(tags: &[&str]) -> ItemFilter {
        ItemFilter {
            library_id: 1,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn the_probe_form_is_correlated_and_the_other_is_not() {
        let materialised = Predicate::build(&filter(&["survey"]), None);
        assert!(materialised.sql.contains("i.id IN (SELECT it.item_id"), "{}", materialised.sql);

        let probed = Predicate::build_with(&filter(&["survey"]), None, TagForm::Probe);
        // Correlated on i.id: that is what lets SQLite walk the sort order and
        // stop at a full page instead of collecting and sorting the whole set.
        assert!(probed.sql.contains("EXISTS (SELECT 1 FROM item_tags it"), "{}", probed.sql);
        assert!(probed.sql.contains("it.item_id = i.id"), "{}", probed.sql);
        // Same bindings either way, so the caller can swap forms freely.
        assert_eq!(materialised.params.len(), probed.params.len());
    }

    #[test]
    fn a_negated_tag_is_never_probed() {
        // `NOT EXISTS` has to be asked of every row in the library, which is
        // the shape the probe exists to avoid.
        let p = Predicate::build_with(&filter(&["-obsolete"]), None, TagForm::Probe);
        assert!(p.sql.contains("NOT IN"), "{}", p.sql);
        assert!(!p.sql.contains("EXISTS"), "{}", p.sql);
    }

    #[test]
    fn only_a_plain_tag_filter_is_eligible() {
        assert!(Predicate::build(&filter(&["survey"]), None).tags_only);
        assert!(Predicate::build(&filter(&["survey", "llm"]), None).tags_only);

        // Anything else narrowing the set means the tag may not be the
        // selective clause, and naming the sort index would be a guess.
        assert!(!Predicate::build(&filter(&[]), None).tags_only, "no tag at all");
        assert!(!Predicate::build(&filter(&["-obsolete"]), None).tags_only, "negated");
        assert!(!Predicate::build(&filter(&["survey", "-old"]), None).tags_only, "mixed");
        assert!(!Predicate::build(&filter(&["survey"]), Some(&[7])).tags_only, "collection too");

        let typed = ItemFilter { item_types: vec!["book".into()], ..filter(&["survey"]) };
        assert!(!Predicate::build(&typed, None).tags_only, "type too");
    }

    #[test]
    fn the_crossover_is_where_the_two_costs_meet() {
        // 131k rows, a page of 100: sqrt(131026 * 100) is about 3620.
        let (rows, window) = (131_026, 100);
        assert!(!should_walk(3_000, window, rows), "below the crossover, materialise");
        assert!(should_walk(4_587, window, rows), "above it, walk");

        // Measured either side of that line on exactly this corpus: a tag on 4
        // items is 0.1ms materialised and 50ms probed; one on 28,709 items is
        // 33ms materialised and 0.2ms probed.
        assert!(!should_walk(4, window, rows));
        assert!(should_walk(28_709, window, rows));
    }

    #[test]
    fn a_deep_page_raises_the_bar() {
        // The walk has to read past the offset too, so the further in the page
        // is, the more common a tag has to be before walking pays.
        let rows = 131_026;
        assert!(should_walk(5_000, 100, rows));
        assert!(!should_walk(5_000, 50_000, rows), "at offset 50k, materialise");
    }

    #[test]
    fn without_statistics_nothing_changes() {
        // A library nobody has analysed keeps the plan that cannot degrade.
        assert!(!should_walk(28_709, 100, 0));
        // And the degenerate inputs a clamped query could still produce.
        assert!(!should_walk(0, 100, 131_026));
        assert!(!should_walk(28_709, 0, 131_026));
    }
}
