//! An exact cache for answers derived from a library's contents.
//!
//! Counting is what a listing actually spends its time on. Picking the page
//! itself reads a hundred rows from an index; counting the matches visits every
//! one of them — 5.8ms of an 8.3ms plain browse on a 131k library, and 13ms of
//! a tag filter. Scrolling recomputes the identical number on every page.
//!
//! **Nothing here is ever stale.** The key carries the library's version, which
//! every write increments, so a cached answer is either from the exact state
//! being queried or it is not used. That is a different bargain from
//! [`crate::cache`], which trades a moment's exactness for a stall it has
//! measured; a total drives how many pages the client thinks exist, and a
//! listing that disagrees with itself is worth more than the milliseconds.
//!
//! The win is therefore only on repeats — but repeats are what browsing is.
//! Reading the version costs 0.0072ms, so a miss pays essentially nothing for
//! having looked.

use std::collections::HashMap;

use parking_lot::Mutex;
use rusqlite::types::Value as SqlValue;

use crate::filter::Predicate;

/// How many distinct queries to remember.
///
/// Small on purpose. A library has a handful of shelves and tags somebody
/// actually browses, and the entries that are not those are `since` deltas and
/// one-off searches that will never be asked again. When it fills, it is
/// cleared rather than evicted by age: keeping a least-recently-used order
/// costs more bookkeeping than the thing being cached, and the next few
/// requests simply pay what they used to pay.
const CAPACITY: usize = 256;

/// A value remembered against the library version it was computed from.
///
/// Generic in the value because the shape of the answer is not the interesting
/// part — the invalidation is. Row counts are `i64`; the sidebar's collection
/// list is a `Vec<Collection>` whose per-collection totals cost the same 27ms
/// however they are spelled.
pub struct Versioned<T> {
    entries: Mutex<HashMap<String, (i64, T)>>,
}

/// The original use, and the common one.
pub type CountCache = Versioned<i64>;

impl<T> Default for Versioned<T> {
    fn default() -> Self {
        Self { entries: Mutex::new(HashMap::new()) }
    }
}

impl<T: Clone> Versioned<T> {
    /// A key identifying exactly this question.
    ///
    /// The predicate's SQL and its bound values together *are* the question, so
    /// there is nothing to keep in step: a filter that changes shape changes
    /// its SQL, and one that changes a value changes its parameters.
    pub fn key(library_id: i64, p: &Predicate) -> String {
        let mut key = format!("{library_id}\u{1}{}", p.sql);
        for param in &p.params {
            key.push('\u{1}');
            match param {
                SqlValue::Integer(i) => key.push_str(&i.to_string()),
                SqlValue::Real(f) => key.push_str(&f.to_string()),
                SqlValue::Text(t) => key.push_str(t),
                SqlValue::Blob(b) => key.push_str(&b.len().to_string()),
                SqlValue::Null => key.push('\0'),
            }
        }
        key
    }

    /// The answer to this question as of `version`, if it was asked before.
    pub fn get(&self, key: &str, version: i64) -> Option<T> {
        match self.entries.lock().get(key) {
            Some((at, value)) if *at == version => Some(value.clone()),
            _ => None,
        }
    }

    pub fn put(&self, key: String, version: i64, count: T) {
        let mut entries = self.entries.lock();
        if entries.len() >= CAPACITY {
            entries.clear();
        }
        entries.insert(key, (version, count));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::query::ItemFilter;

    fn predicate(tag: &str) -> Predicate {
        Predicate::build(
            &ItemFilter { library_id: 1, tags: vec![tag.to_string()], ..Default::default() },
            None,
        )
    }

    #[test]
    fn a_write_retires_the_answer() {
        let cache = CountCache::default();
        let key = CountCache::key(1, &predicate("survey"));
        cache.put(key.clone(), 7, 100);

        assert_eq!(cache.get(&key, 7), Some(100));
        // The library version moved, so the count is from a state nobody is
        // asking about. There is no staleness window here at all.
        assert_eq!(cache.get(&key, 8), None);
    }

    #[test]
    fn different_questions_do_not_share_an_answer() {
        let cache = CountCache::default();
        let survey = CountCache::key(1, &predicate("survey"));
        let review = CountCache::key(1, &predicate("review"));
        // Same SQL, different bound value: the parameters have to be part of
        // the key or every tag would read the first tag's count.
        assert_ne!(survey, review);

        cache.put(survey.clone(), 1, 100);
        assert_eq!(cache.get(&review, 1), None);
    }

    #[test]
    fn libraries_do_not_share_an_answer() {
        // Two libraries have independent version counters, so without the id
        // in the key one could answer with the other's count at a version that
        // happens to match.
        assert_ne!(CountCache::key(1, &predicate("survey")), CountCache::key(2, &predicate("survey")));
    }

    #[test]
    fn filling_up_costs_a_recount_and_not_a_wrong_count() {
        let cache = CountCache::default();
        let kept = CountCache::key(1, &predicate("kept"));
        cache.put(kept.clone(), 1, 42);
        for i in 0..CAPACITY {
            cache.put(format!("filler-{i}"), 1, i as i64);
        }
        // Whatever survived, nothing came back wrong.
        assert!(matches!(cache.get(&kept, 1), None | Some(42)));
        assert_eq!(cache.entries.lock().len(), 1, "cleared rather than grown");
    }
}
