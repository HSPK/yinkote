//! A caching decorator for the expensive tag aggregates.
//!
//! Counting tags means grouping over every `item_tags` row and checking each
//! item's trash flag — ~130 ms on a 100k-item library, and the sidebar asks for
//! it on every navigation.
//!
//! The cache key is the library's version counter, which every write increments.
//! That makes invalidation exact and free: there is no TTL to tune, and a stale
//! count cannot be served because a changed library has a different key.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use yk_core::model::Tag;
use yk_core::ports::{LibraryRepository, TagRepository};
use yk_core::query::ItemFilter;
use yk_core::Result;

/// Entries for one library at one version. Replaced wholesale when the library
/// changes, which also bounds memory without an eviction policy.
#[derive(Default)]
struct Generation {
    version: i64,
    entries: HashMap<String, Vec<Tag>>,
}

pub struct CachingTagRepository {
    inner: Arc<dyn TagRepository>,
    libraries: Arc<dyn LibraryRepository>,
    cache: Mutex<HashMap<i64, Generation>>,
}

impl CachingTagRepository {
    pub fn new(inner: Arc<dyn TagRepository>, libraries: Arc<dyn LibraryRepository>) -> Self {
        Self { inner, libraries, cache: Mutex::new(HashMap::new()) }
    }

    fn get(&self, library_id: i64, version: i64, key: &str) -> Option<Vec<Tag>> {
        let cache = self.cache.lock();
        let generation = cache.get(&library_id)?;
        (generation.version == version).then(|| generation.entries.get(key).cloned())?
    }

    fn put(&self, library_id: i64, version: i64, key: String, value: Vec<Tag>) {
        let mut cache = self.cache.lock();
        let generation = cache.entry(library_id).or_default();
        if generation.version != version {
            *generation = Generation { version, entries: HashMap::new() };
        }
        generation.entries.insert(key, value);
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

#[async_trait]
impl TagRepository for CachingTagRepository {
    async fn list(&self, library_id: i64, prefix: Option<&str>, limit: u32) -> Result<Vec<Tag>> {
        let version = self.libraries.version(library_id).await?;
        let key = format!("list|{limit}|{}", prefix.unwrap_or(""));
        if let Some(hit) = self.get(library_id, version, &key) {
            return Ok(hit);
        }
        let value = self.inner.list(library_id, prefix, limit).await?;
        self.put(library_id, version, key, value.clone());
        Ok(value)
    }

    async fn facets(&self, filter: &ItemFilter, limit: u32) -> Result<Vec<Tag>> {
        let version = self.libraries.version(filter.library_id).await?;
        let key = Self::key("facets", filter, limit);
        if let Some(hit) = self.get(filter.library_id, version, &key) {
            return Ok(hit);
        }
        let value = self.inner.facets(filter, limit).await?;
        self.put(filter.library_id, version, key, value.clone());
        Ok(value)
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

    #[derive(Default)]
    struct CountingTags {
        list_calls: AtomicUsize,
        facet_calls: AtomicUsize,
    }

    #[async_trait]
    impl TagRepository for CountingTags {
        async fn list(&self, _: i64, _: Option<&str>, _: u32) -> Result<Vec<Tag>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
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
        let tags = Arc::new(CountingTags::default());
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

    #[tokio::test]
    async fn a_write_invalidates_everything() {
        let (cache, tags, libs) = setup();
        cache.list(1, None, 10).await.unwrap();
        libs.version.fetch_add(1, Ordering::SeqCst);
        cache.list(1, None, 10).await.unwrap();
        assert_eq!(tags.list_calls.load(Ordering::SeqCst), 2);
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
