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

    /// Every attachment in the library, with the item it belongs to.
    ///
    /// Paired here rather than fetched per file: a library's attachments run to
    /// thousands, and one query per file is the difference between a page that
    /// opens and one that does not.
    async fn attachments(
        &self,
        library_id: i64,
        limit: u32,
        offset: u32,
    ) -> Result<Page<(Item, Option<Item>)>>;

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
pub trait ConversationRepository: Send + Sync {
    async fn list(&self, library_id: i64, limit: u32) -> Result<Vec<Conversation>>;
    async fn get(&self, library_id: i64, key: &Key) -> Result<Conversation>;
    async fn create(&self, library_id: i64, title: &str, scope: Option<&str>)
        -> Result<Conversation>;
    /// Change a conversation's title, its scope, or both.
    ///
    /// One method rather than one per field: a conversation has two mutable
    /// properties and two nearly identical UPDATE statements would drift.
    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: ConversationPatch,
    ) -> Result<Conversation>;
    async fn delete(&self, library_id: i64, key: &Key) -> Result<u64>;

    async fn messages(&self, library_id: i64, key: &Key) -> Result<Vec<Message>>;

    /// One page of a thread, newest first by default.
    ///
    /// A conversation is not bounded — a working thread runs to hundreds of
    /// messages and one tool result can be a hundred kilobytes — so opening
    /// one must not depend on how long it has been going. `before` is a
    /// message id: everything older than it, most recent first.
    ///
    /// The page comes back in reading order, because that is how it is drawn.
    async fn messages_page(
        &self,
        library_id: i64,
        key: &Key,
        limit: u32,
        before: Option<i64>,
    ) -> Result<MessagePage>;
    async fn append(
        &self,
        library_id: i64,
        key: &Key,
        draft: MessageDraft,
    ) -> Result<Message>;

    /// The conversations that mention a paper, most recent first.
    ///
    /// Asked from the paper, not from the chat: standing on something you are
    /// reading, "what did I already work out about this" is a question the
    /// library should be able to answer.
    async fn mentioning(&self, library_id: i64, item: &Key) -> Result<Vec<Conversation>>;
}

#[async_trait]
pub trait SmartCollectionRepository: Send + Sync {
    async fn list(&self, library_id: i64) -> Result<Vec<SmartCollection>>;
    async fn get(&self, library_id: i64, key: &Key) -> Result<SmartCollection>;
    async fn create(&self, library_id: i64, draft: SmartCollectionDraft)
        -> Result<SmartCollection>;
    async fn update(
        &self,
        library_id: i64,
        key: &Key,
        patch: SmartCollectionPatch,
    ) -> Result<SmartCollection>;
    async fn delete(&self, library_id: i64, key: &Key) -> Result<u64>;
}

#[async_trait]
pub trait TagRepository: Send + Sync {
    async fn list(&self, library_id: i64, prefix: Option<&str>, limit: u32) -> Result<Vec<Tag>>;
    /// How many distinct tags the library has.
    ///
    /// Separate from `list` because the two questions cost wildly different
    /// amounts: this is a row count on `tags`, while listing groups over every
    /// `item_tags` row to attach a count to each name. The statistics endpoint
    /// asked for the whole list and took its length — a 200ms aggregate over a
    /// hundred thousand items, to learn the number 33.
    async fn count(&self, library_id: i64) -> Result<i64>;
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
    /// The items whose meaning is closest to one already in the library, with
    /// their cosine similarity, best first.
    ///
    /// Empty when the item has not been embedded yet — which is not the same
    /// claim as "nothing is similar", and callers must not present it as one.
    async fn similar(&self, library_id: i64, key: &Key, k: usize) -> Result<Vec<(Key, f32)>>;
    /// Compute and store embeddings for queued documents. Returns how many
    /// were processed; call in a loop until it returns 0.
    async fn embed_pending(&self, batch: u32) -> Result<u32>;
    /// Rebuild every derived structure for a library from the items table.
    async fn reindex(&self, library_id: i64) -> Result<u64>;
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
