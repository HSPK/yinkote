//! Ports — the interfaces the application layer depends on.
//!
//! Everything here is an abstraction; concrete adapters live in `yk-store`,
//! `yk-search` and `yk-plugin`. This is what keeps the dependency arrows
//! pointing inwards.

use async_trait::async_trait;
use serde_json::Value;

use crate::model::*;
use crate::plugin::*;
use crate::query::*;
use crate::{Key, Result};

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<Library>>;
    async fn get(&self, id: i64) -> Result<Library>;
    /// Monotonic per-library version counter, the basis of delta sync.
    async fn version(&self, id: i64) -> Result<i64>;
}

#[async_trait]
pub trait ItemRepository: Send + Sync {
    async fn get(&self, library_id: i64, key: &Key) -> Result<Item>;
    async fn get_many(&self, library_id: i64, keys: &[Key]) -> Result<Vec<Item>>;
    async fn list(&self, query: &ItemQuery) -> Result<Page<Item>>;
    async fn children(&self, library_id: i64, parent: &Key) -> Result<Vec<Item>>;

    async fn create(&self, library_id: i64, draft: ItemDraft) -> Result<Item>;
    /// Batch create. Returns one result per input, so a single bad row does not
    /// fail the whole request.
    async fn create_many(
        &self,
        library_id: i64,
        drafts: Vec<ItemDraft>,
    ) -> Result<Vec<Result<Item>>>;
    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: ItemPatch,
        if_version: Option<i64>,
    ) -> Result<Item>;

    async fn set_trashed(&self, library_id: i64, keys: &[Key], trashed: bool) -> Result<u64>;
    async fn delete(&self, library_id: i64, keys: &[Key]) -> Result<u64>;
    async fn empty_trash(&self, library_id: i64) -> Result<u64>;

    async fn add_to_collection(
        &self,
        library_id: i64,
        collection: &Key,
        keys: &[Key],
    ) -> Result<u64>;
    async fn remove_from_collection(
        &self,
        library_id: i64,
        collection: &Key,
        keys: &[Key],
    ) -> Result<u64>;

    /// Existing items whose fingerprint matches any of `fingerprints`.
    async fn find_by_fingerprint(
        &self,
        library_id: i64,
        fingerprints: &[String],
    ) -> Result<Vec<Item>>;

    /// Streaming-friendly scan used to (re)build the search index.
    async fn scan(&self, library_id: i64, after_rowid: i64, limit: u32)
        -> Result<(Vec<Item>, i64)>;

    async fn count(&self, filter: &ItemFilter) -> Result<i64>;
}

#[async_trait]
pub trait CollectionRepository: Send + Sync {
    async fn list(&self, library_id: i64) -> Result<Vec<Collection>>;
    async fn get(&self, library_id: i64, key: &Key) -> Result<Collection>;
    async fn create(&self, library_id: i64, draft: CollectionDraft) -> Result<Collection>;
    async fn update(&self, library_id: i64, key: &Key, patch: CollectionPatch)
        -> Result<Collection>;
    async fn delete(&self, library_id: i64, key: &Key, recursive: bool) -> Result<u64>;
    /// `key` plus every descendant, used for recursive listing.
    async fn descendants(&self, library_id: i64, key: &Key) -> Result<Vec<Key>>;
}

#[async_trait]
pub trait TagRepository: Send + Sync {
    async fn list(&self, library_id: i64, prefix: Option<&str>, limit: u32) -> Result<Vec<Tag>>;
    async fn rename(&self, library_id: i64, from: &str, to: &str) -> Result<u64>;
    async fn delete(&self, library_id: i64, name: &str) -> Result<u64>;
    async fn set_color(&self, library_id: i64, name: &str, color: Option<&str>) -> Result<()>;
    /// Tags co-occurring with the current filter, for faceted narrowing.
    async fn facets(&self, filter: &ItemFilter, limit: u32) -> Result<Vec<Tag>>;
}

/// Simple string key/value store for settings and plugin state.
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Value>>;
    async fn set(&self, key: &str, value: &Value) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn list(&self, prefix: &str) -> Result<Vec<(String, Value)>>;
}

/// Retrieval and embedding lifecycle.
///
/// Note that the lexical indexes are maintained *transactionally by the store*
/// alongside every item write — that is the only way to guarantee they cannot
/// drift. This port therefore owns querying plus the asynchronous embedding
/// worker, not index mutation.
#[async_trait]
pub trait SearchIndex: Send + Sync {
    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchHit>>;
    async fn stats(&self) -> Result<SearchStats>;
    /// Compute and store embeddings for queued documents. Returns how many
    /// were processed; call in a loop until it returns 0.
    async fn embed_pending(&self, batch: u32) -> Result<u32>;
    /// Rebuild every derived structure for a library from the items table.
    async fn reindex(&self, library_id: i64) -> Result<u64>;
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn id(&self) -> &str;
    fn dimensions(&self) -> usize;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Everything the server needs from the plugin subsystem.
#[async_trait]
pub trait PluginHost: Send + Sync {
    async fn list(&self) -> Vec<PluginStatus>;
    async fn get(&self, id: &str) -> Result<PluginStatus>;
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginStatus>;
    /// Rescan the plugin directories and restart changed plugins.
    async fn reload(&self) -> Result<()>;
    /// Aggregated contributions of all ready plugins.
    async fn contributions(&self) -> Contributions;
    /// Direct JSON-RPC call into one plugin.
    async fn call(&self, plugin_id: &str, method: &str, params: Value) -> Result<Value>;
    /// Fan out to every subscriber; never fails as a whole.
    async fn dispatch(&self, event: HookEvent) -> Vec<HookOutcome>;
    async fn shutdown(&self);
}

/// The reverse direction: what a plugin may ask the host to do.
/// Implemented by the server so `yk-plugin` stays free of business logic.
#[async_trait]
pub trait HostApi: Send + Sync {
    async fn invoke(
        &self,
        plugin_id: &str,
        granted: &[Permission],
        method: &str,
        params: Value,
    ) -> Result<Value>;
}
