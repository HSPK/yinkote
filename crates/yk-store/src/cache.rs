//! A caching decorator for the expensive tag aggregates.
//!
//! Counting tags means grouping over every `item_tags` row and checking each
//! item's trash flag — ~200 ms on a 100k-item library, and the sidebar asks for
//! it on every navigation.
//!
//! The cache key is the library's version counter, which every write increments.
//! That makes invalidation exact and free: there is no TTL to tune.
//!
//! Exactness alone was not enough, though. Every write retires the generation,
//! so the first read after *any* edit paid the full 200 ms — measured at 213,
//! 243 and 255 ms against a 100k library, with the warm reads at 0.6 ms. In
//! practice that meant the sidebar froze briefly whenever anything was saved.
//!
//! So a superseded answer is served immediately and refreshed behind the
//! request — but only when recomputing it is actually slow. On a small library
//! the count takes about a millisecond, and trading exactness for nothing is a
//! bad trade; on a large one it takes a quarter of a second, and nobody notices
//! a tag reading 28,597 instead of 28,598 for a moment while everybody notices
//! the stall. The cache remembers how long the last computation took and
//! decides on that, so the behaviour is right at both ends without a setting to
//! get wrong.
//!
//! Nothing is served stale on a cold cache: with no previous answer to show,
//! correctness is the only option.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use yk_core::model::Tag;
use yk_core::ports::{LibraryRepository, TagRepository};
use yk_core::query::ItemFilter;
use yk_core::Result;

/// Below this, recomputing is cheaper than reasoning about staleness.
const SLOW_ENOUGH_TO_DEFER: Duration = Duration::from_millis(20);

/// A cached answer and what it cost to produce.
#[derive(Clone)]
struct Entry {
    tags: Vec<Tag>,
    took: Duration,
}

/// Entries for one library at one version. Replaced wholesale when the library
/// changes, which also bounds memory without an eviction policy.
#[derive(Default)]
struct Generation {
    version: i64,
    entries: HashMap<String, Entry>,
}

/// What the cache had to say about a key.
enum Cached {
    /// Computed at the current version.
    Fresh(Vec<Tag>),
    /// Computed at an earlier version; correct then, near enough now.
    Stale(Entry),
    Missing,
}

/// The shared part, so a background refresh can outlive the call that started
/// it without the repository having to know about its own `Arc`.
#[derive(Default)]
struct CacheState {
    entries: Mutex<HashMap<i64, Generation>>,
    /// Keys with a refresh already in flight, so a burst of readers triggers
    /// one recomputation rather than one each.
    refreshing: Mutex<HashMap<i64, Vec<String>>>,
}

pub struct CachingTagRepository {
    inner: Arc<dyn TagRepository>,
    libraries: Arc<dyn LibraryRepository>,
    state: Arc<CacheState>,
}

impl CachingTagRepository {
    pub fn new(inner: Arc<dyn TagRepository>, libraries: Arc<dyn LibraryRepository>) -> Self {
        Self { inner, libraries, state: Arc::new(CacheState::default()) }
    }

    /// Fingerprint of everything that can change the answer.
    fn key(prefix: &str, filter: &ItemFilter, limit: u32) -> String {
        let mut tags = filter.tags.clone();
        tags.sort();
        let mut types = filter.item_types.clone();
        types.sort();
        format!(
            "{prefix}|{limit}|{:?}|{}|{:?}|{:?}|{:?}|{}",
            filter.collection.as_ref().map(|k| k.as_str()),
            filter.recursive,
            filter.trash,
            tags,
            types,
            filter.top_level_only,
        )
    }
}

/// Run a computation and remember what it cost.
async fn timed(
    work: impl std::future::Future<Output = Result<Vec<Tag>>>,
) -> Result<Entry> {
    let started = Instant::now();
    let tags = work.await?;
    Ok(Entry { tags, took: started.elapsed() })
}

impl CacheState {
    fn get(&self, library_id: i64, version: i64, key: &str) -> Cached {
        let entries = self.entries.lock();
        let Some(generation) = entries.get(&library_id) else { return Cached::Missing };
        match generation.entries.get(key) {
            None => Cached::Missing,
            Some(entry) if generation.version == version => Cached::Fresh(entry.tags.clone()),
            Some(entry) => Cached::Stale(entry.clone()),
        }
    }

    fn put(&self, library_id: i64, version: i64, key: String, value: Entry) {
        let mut entries = self.entries.lock();
        let generation = entries.entry(library_id).or_default();
        // A refresh that finishes after a newer write must not resurrect the
        // generation it was computed for.
        if generation.version > version {
            return;
        }
        if generation.version != version {
            *generation = Generation { version, entries: HashMap::new() };
        }
        generation.entries.insert(key, value);
    }

    /// Claim the right to refresh a key, or find that someone already has it.
    fn claim(&self, library_id: i64, key: &str) -> bool {
        let mut busy = self.refreshing.lock();
        let keys = busy.entry(library_id).or_default();
        if keys.iter().any(|k| k == key) {
            return false;
        }
        keys.push(key.to_string());
        true
    }

    fn release(&self, library_id: i64, key: &str) {
        if let Some(keys) = self.refreshing.lock().get_mut(&library_id) {
            keys.retain(|k| k != key);
        }
    }
}

impl CachingTagRepository {
    /// Serve the best answer available, and make sure a better one is coming.
    ///
    /// Fresh is returned as-is. Stale is returned *and* a refresh is started,
    /// so the next reader gets the new number without this one having waited
    /// for it. Missing has nothing to show, so it computes and waits — a cold
    /// cache must be correct, not merely quick.
    async fn answer<F, Fut>(
        &self,
        library_id: i64,
        version: i64,
        key: String,
        compute: F,
    ) -> Result<Vec<Tag>>
    where
        F: Fn(Arc<dyn TagRepository>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Vec<Tag>>> + Send + 'static,
    {
        match self.state.get(library_id, version, &key) {
            Cached::Fresh(hit) => return Ok(hit),
            // Cheap to redo: stay exact rather than buy back a millisecond.
            Cached::Stale(entry) if entry.took < SLOW_ENOUGH_TO_DEFER => {}
            Cached::Stale(entry) => {
                if self.state.claim(library_id, &key) {
                    let state = self.state.clone();
                    let inner = self.inner.clone();
                    tokio::spawn(async move {
                        if let Ok(value) = timed(compute(inner)).await {
                            state.put(library_id, version, key.clone(), value);
                        }
                        state.release(library_id, &key);
                    });
                }
                return Ok(entry.tags);
            }
            Cached::Missing => {}
        }

        let entry = timed(compute(self.inner.clone())).await?;
        self.state.put(library_id, version, key, entry.clone());
        Ok(entry.tags)
    }
}

#[async_trait]
impl TagRepository for CachingTagRepository {
    async fn list(&self, library_id: i64, prefix: Option<&str>, limit: u32) -> Result<Vec<Tag>> {
        let version = self.libraries.version(library_id).await?;
        let key = format!("list|{limit}|{}", prefix.unwrap_or(""));
        let prefix = prefix.map(str::to_string);

        self.answer(library_id, version, key, move |inner| {
            let prefix = prefix.clone();
            async move { inner.list(library_id, prefix.as_deref(), limit).await }
        })
        .await
    }

    async fn facets(&self, filter: &ItemFilter, limit: u32) -> Result<Vec<Tag>> {
        let version = self.libraries.version(filter.library_id).await?;
        let key = Self::key("facets", filter, limit);
        let library_id = filter.library_id;
        let filter = filter.clone();

        self.answer(library_id, version, key, move |inner| {
            let filter = filter.clone();
            async move { inner.facets(&filter, limit).await }
        })
        .await
    }

    // Mutations pass straight through: each bumps the library version, which
    // retires the whole generation on the next read.
    async fn rename(&self, library_id: i64, from: &str, to: &str) -> Result<u64> {
        self.inner.rename(library_id, from, to).await
    }

    async fn delete(&self, library_id: i64, name: &str) -> Result<u64> {
        self.inner.delete(library_id, name).await
    }

    async fn set_color(&self, library_id: i64, name: &str, color: Option<&str>) -> Result<()> {
        self.inner.set_color(library_id, name, color).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use yk_core::model::Library;

    /// Long enough to count as expensive, short enough not to slow the suite.
    const SLOW_COMPUTE: Duration = Duration::from_millis(35);

    #[derive(Default)]
    struct CountingTags {
        list_calls: AtomicUsize,
        facet_calls: AtomicUsize,
        slow: bool,
    }

    #[async_trait]
    impl TagRepository for CountingTags {
        async fn list(&self, _: i64, _: Option<&str>, _: u32) -> Result<Vec<Tag>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            if self.slow {
                tokio::time::sleep(SLOW_COMPUTE).await;
            }
            Ok(vec![Tag { name: "a".into(), color: None, count: 1, r#type: 0 }])
        }
        async fn facets(&self, _: &ItemFilter, _: u32) -> Result<Vec<Tag>> {
            self.facet_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![Tag { name: "b".into(), color: None, count: 2, r#type: 0 }])
        }
        async fn rename(&self, _: i64, _: &str, _: &str) -> Result<u64> {
            Ok(0)
        }
        async fn delete(&self, _: i64, _: &str) -> Result<u64> {
            Ok(0)
        }
        async fn set_color(&self, _: i64, _: &str, _: Option<&str>) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeLibraries {
        version: AtomicUsize,
    }

    #[async_trait]
    impl LibraryRepository for FakeLibraries {
        async fn list(&self) -> Result<Vec<Library>> {
            Ok(Vec::new())
        }
        async fn get(&self, _: i64) -> Result<Library> {
            Err(yk_core::Error::not_found("library"))
        }
        async fn version(&self, _: i64) -> Result<i64> {
            Ok(self.version.load(Ordering::SeqCst) as i64)
        }
    }

    fn setup() -> (CachingTagRepository, Arc<CountingTags>, Arc<FakeLibraries>) {
        build(false)
    }

    /// A repository whose answers cost enough to be worth deferring.
    fn slow_setup() -> (CachingTagRepository, Arc<CountingTags>, Arc<FakeLibraries>) {
        build(true)
    }

    fn build(slow: bool) -> (CachingTagRepository, Arc<CountingTags>, Arc<FakeLibraries>) {
        let tags = Arc::new(CountingTags { slow, ..Default::default() });
        let libs = Arc::new(FakeLibraries::default());
        (CachingTagRepository::new(tags.clone(), libs.clone()), tags, libs)
    }

    #[tokio::test]
    async fn repeated_reads_hit_the_cache() {
        let (cache, tags, _) = setup();
        for _ in 0..5 {
            assert_eq!(cache.list(1, None, 10).await.unwrap().len(), 1);
        }
        assert_eq!(tags.list_calls.load(Ordering::SeqCst), 1);
    }

    /// Let any spawned refresh finish before asserting about it.
    async fn settle() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn a_cheap_answer_is_recomputed_exactly_after_a_write() {
        // Most libraries are small and the count takes about a millisecond.
        // Trading exactness for that would be a bad bargain.
        let (cache, tags, libs) = setup();
        cache.list(1, None, 10).await.unwrap();
        libs.version.fetch_add(1, Ordering::SeqCst);
        cache.list(1, None, 10).await.unwrap();
        assert_eq!(tags.list_calls.load(Ordering::SeqCst), 2, "recomputed inline");
    }

    #[tokio::test]
    async fn an_expensive_answer_is_served_stale_and_refreshed_behind() {
        let (cache, tags, libs) = slow_setup();
        cache.list(1, None, 10).await.unwrap();

        libs.version.fetch_add(1, Ordering::SeqCst);
        let started = std::time::Instant::now();
        cache.list(1, None, 10).await.unwrap();
        assert!(
            started.elapsed() < SLOW_COMPUTE,
            "the reader was not made to wait: {:?}",
            started.elapsed()
        );

        settle().await;
        assert_eq!(tags.list_calls.load(Ordering::SeqCst), 2, "a refresh happened behind it");
    }

    #[tokio::test]
    async fn a_burst_of_readers_triggers_one_refresh() {
        let (cache, tags, libs) = slow_setup();
        cache.list(1, None, 10).await.unwrap();
        libs.version.fetch_add(1, Ordering::SeqCst);

        for _ in 0..5 {
            cache.list(1, None, 10).await.unwrap();
        }
        settle().await;
        assert_eq!(tags.list_calls.load(Ordering::SeqCst), 2, "five readers, one recomputation");
    }

    #[tokio::test]
    async fn a_cold_cache_waits_rather_than_inventing_an_answer() {
        // Nothing to show means nothing to serve stale; correctness first.
        let (cache, tags, _) = setup();
        assert_eq!(cache.facets(&ItemFilter { library_id: 1, ..Default::default() }, 10)
            .await
            .unwrap()
            .len(), 1);
        assert_eq!(tags.facet_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_late_refresh_cannot_resurrect_an_old_generation() {
        // A refresh started before a write may land after it. Writing it back
        // would replace correct data with data computed from an older library.
        let state = CacheState::default();
        let entry = |name: &str| Entry {
            tags: vec![Tag { name: name.into(), color: None, count: 1, r#type: 0 }],
            took: Duration::ZERO,
        };
        state.put(1, 5, "k".into(), entry("new"));
        state.put(1, 4, "k".into(), entry("old"));

        match state.get(1, 5, "k") {
            Cached::Fresh(tags) => assert_eq!(tags[0].name, "new"),
            _ => panic!("the newer generation must survive"),
        }
    }

    #[tokio::test]
    async fn different_filters_are_cached_separately() {
        let (cache, tags, _) = setup();
        let base = ItemFilter { library_id: 1, ..Default::default() };
        let scoped = ItemFilter { library_id: 1, tags: vec!["x".into()], ..Default::default() };
        cache.facets(&base, 10).await.unwrap();
        cache.facets(&scoped, 10).await.unwrap();
        cache.facets(&base, 10).await.unwrap();
        assert_eq!(tags.facet_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn filter_key_ignores_tag_order() {
        let a = ItemFilter {
            library_id: 1,
            tags: vec!["x".into(), "y".into()],
            ..Default::default()
        };
        let b = ItemFilter {
            library_id: 1,
            tags: vec!["y".into(), "x".into()],
            ..Default::default()
        };
        assert_eq!(
            CachingTagRepository::key("facets", &a, 10),
            CachingTagRepository::key("facets", &b, 10)
        );
    }

    #[tokio::test]
    async fn different_limits_are_cached_separately() {
        let (cache, tags, _) = setup();
        cache.list(1, None, 10).await.unwrap();
        cache.list(1, None, 20).await.unwrap();
        assert_eq!(tags.list_calls.load(Ordering::SeqCst), 2);
    }
}
