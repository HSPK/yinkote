//! SQLite persistence adapters.
//!
//! Exposes one concrete implementation per repository port plus a [`Store`]
//! aggregate that owns the connection pool and hands out `Arc`'d ports.

mod cache;
mod collections;
mod conversations;
mod db;
pub mod filter;
pub mod downloads;
pub mod graph;
pub mod relations;
pub mod index;
mod items;
mod smart;

use std::path::Path;
use std::sync::Arc;

pub use cache::CachingTagRepository;
pub use collections::{SqliteCollectionRepository, SqliteLibraryRepository, SqliteTagRepository};
pub use db::{sql_err, write_tx, Db, Pool, PooledConn};
pub use items::{SqliteItemRepository, SqliteSettingsRepository};
pub use conversations::SqliteConversationRepository;
pub use graph::{GraphRepository, Neighbour, Relation, SqliteGraphRepository};
pub use downloads::{Download, DownloadDraft, DownloadQueue, SqliteDownloadQueue};
pub use relations::{Citation, CitationDraft, RelationRepository, SqliteRelationRepository};
pub use smart::SqliteSmartCollectionRepository;

use yk_core::ports::*;
use yk_core::Result;

/// The default library's name.
///
/// Deliberately language-neutral: the sidebar renders its own translated label,
/// so a name baked into the database in one language would only ever leak into
/// exports and API responses read by the other.
pub const DEFAULT_LIBRARY_NAME: &str = "My Library";

/// Composition root for persistence. Cloning is cheap.
#[derive(Clone)]
pub struct Store {
    db: Db,
    pub libraries: Arc<dyn LibraryRepository>,
    pub items: Arc<dyn ItemRepository>,
    pub collections: Arc<dyn CollectionRepository>,
    pub tags: Arc<dyn TagRepository>,
    pub smart: Arc<dyn SmartCollectionRepository>,
    pub conversations: Arc<dyn ConversationRepository>,
    pub graph: Arc<dyn GraphRepository>,
    pub relations: Arc<dyn RelationRepository>,
    pub downloads: Arc<dyn DownloadQueue>,
    pub settings: Arc<dyn SettingsRepository>,
    /// Concrete handle for operations outside the port surface (index rebuild).
    items_impl: SqliteItemRepository,
    /// Id of the personal library, created on first run.
    pub default_library: i64,
}

impl Store {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let db = Db::open(path)?;
        let default_library = SqliteLibraryRepository::ensure_default(&db, DEFAULT_LIBRARY_NAME)?;
        let items_impl = SqliteItemRepository::new(db.clone());
        let libraries: Arc<dyn LibraryRepository> =
            Arc::new(SqliteLibraryRepository::new(db.clone()));
        // Tag aggregates are read on every navigation and are pure derivations
        // of the library version, so they are worth memoising.
        let tags: Arc<dyn TagRepository> = Arc::new(CachingTagRepository::new(
            Arc::new(SqliteTagRepository::new(db.clone())),
            libraries.clone(),
        ));
        Ok(Self {
            libraries,
            items: Arc::new(items_impl.clone()),
            collections: Arc::new(SqliteCollectionRepository::new(db.clone())),
            tags,
            smart: Arc::new(SqliteSmartCollectionRepository::new(db.clone())),
            conversations: Arc::new(SqliteConversationRepository::new(db.clone())),
            graph: Arc::new(SqliteGraphRepository::new(db.clone())),
            relations: Arc::new(SqliteRelationRepository::new(db.clone())),
            downloads: Arc::new(SqliteDownloadQueue::new(db.clone())),
            settings: Arc::new(SqliteSettingsRepository::new(db.clone())),
            items_impl,
            default_library,
            db,
        })
    }

    /// In-memory store, used by tests.
    pub fn in_memory() -> Result<Self> {
        Self::open(None)
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Recreate every derived search structure for a library.
    pub async fn rebuild_index(&self, library_id: i64) -> Result<u64> {
        self.items_impl.rebuild_index(library_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::*;
    use yk_core::query::*;

    fn store() -> Store {
        Store::in_memory().unwrap()
    }

    fn article(title: &str) -> ItemDraft {
        ItemDraft::new("journalArticle")
            .with_field("title", title)
            .with_field("date", "2023-05-01")
            .with_creator(Creator {
                last_name: Some("Zhang".into()),
                first_name: Some("Wei".into()),
                ..Default::default()
            })
    }

    fn fields(v: serde_json::Value) -> Fields {
        v.as_object().unwrap().clone()
    }

    #[tokio::test]
    async fn create_and_read_round_trips() {
        let s = store();
        let lib = s.default_library;
        let created = s.items.create(lib, article("Attention Is All You Need")).await.unwrap();
        assert_eq!(created.title(), "Attention Is All You Need");
        assert_eq!(created.version, 1);

        let fetched = s.items.get(lib, &created.key).await.unwrap();
        assert_eq!(fetched.key, created.key);
        assert_eq!(fetched.creators.len(), 1);
        assert_eq!(fetched.year(), Some(2023));
    }

    #[tokio::test]
    async fn rejects_unknown_item_type() {
        let s = store();
        let err = s.items.create(s.default_library, ItemDraft::new("nope")).await.unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Invalid);
    }

    #[tokio::test]
    async fn version_increases_monotonically() {
        let s = store();
        let lib = s.default_library;
        let a = s.items.create(lib, article("A")).await.unwrap();
        let b = s.items.create(lib, article("B")).await.unwrap();
        assert!(b.version > a.version);
        assert_eq!(s.libraries.version(lib).await.unwrap(), b.version);
    }

    #[tokio::test]
    async fn optimistic_locking_detects_conflict() {
        let s = store();
        let lib = s.default_library;
        let item = s.items.create(lib, article("A")).await.unwrap();
        let patch = ItemPatch { fields: Some(fields(serde_json::json!({"title":"B"}))), ..Default::default() };
        s.items.update(lib, &item.key, patch.clone(), Some(item.version)).await.unwrap();
        let err = s.items.update(lib, &item.key, patch, Some(item.version)).await.unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::VersionConflict);
    }

    #[tokio::test]
    async fn patch_merges_and_null_clears() {
        let s = store();
        let lib = s.default_library;
        let item =
            s.items.create(lib, article("A").with_field("abstractNote", "hello")).await.unwrap();
        let patch = ItemPatch {
            fields: Some(fields(serde_json::json!({"abstractNote": null, "volume": "12"}))),
            ..Default::default()
        };
        let out = s.items.update(lib, &item.key, patch, None).await.unwrap();
        assert!(out.field("abstractNote").is_none());
        assert_eq!(out.field("volume"), Some("12"));
        assert_eq!(out.title(), "A", "unrelated fields survive");
    }

    #[tokio::test]
    async fn batch_create_isolates_failures() {
        let s = store();
        let results = s
            .items
            .create_many(s.default_library, vec![article("ok"), ItemDraft::new("bogus")])
            .await
            .unwrap();
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }

    #[tokio::test]
    async fn trash_and_restore() {
        let s = store();
        let lib = s.default_library;
        let item = s.items.create(lib, article("A")).await.unwrap();
        s.items.set_trashed(lib, std::slice::from_ref(&item.key), true).await.unwrap();

        let visible = s.items.count(&ItemFilter { library_id: lib, ..Default::default() }).await.unwrap();
        assert_eq!(visible, 0);

        let trashed = s
            .items
            .count(&ItemFilter { library_id: lib, trash: TrashScope::Only, ..Default::default() })
            .await
            .unwrap();
        assert_eq!(trashed, 1);

        s.items.set_trashed(lib, &[item.key], false).await.unwrap();
        assert_eq!(
            s.items.count(&ItemFilter { library_id: lib, ..Default::default() }).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn collections_nest_and_filter_recursively() {
        let s = store();
        let lib = s.default_library;
        let parent = s
            .collections
            .create(lib, CollectionDraft { name: "ML".into(), ..Default::default() })
            .await
            .unwrap();
        let child = s
            .collections
            .create(
                lib,
                CollectionDraft {
                    name: "Diffusion".into(),
                    parent_key: Some(parent.key.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut draft = article("Nested");
        draft.collections = vec![child.key.clone()];
        s.items.create(lib, draft).await.unwrap();

        let shallow = s
            .items
            .count(&ItemFilter {
                library_id: lib,
                collection: Some(parent.key.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(shallow, 0);

        let deep = s
            .items
            .count(&ItemFilter {
                library_id: lib,
                collection: Some(parent.key),
                recursive: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(deep, 1);
    }

    #[tokio::test]
    async fn collection_cycle_is_rejected() {
        let s = store();
        let lib = s.default_library;
        let a = s
            .collections
            .create(lib, CollectionDraft { name: "A".into(), ..Default::default() })
            .await
            .unwrap();
        let b = s
            .collections
            .create(
                lib,
                CollectionDraft {
                    name: "B".into(),
                    parent_key: Some(a.key.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let err = s
            .collections
            .update(lib, &a.key, CollectionPatch { parent_key: Some(Some(b.key)), ..Default::default() })
            .await
            .unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Invalid);
    }

    #[tokio::test]
    async fn tags_filter_positively_and_negatively() {
        let s = store();
        let lib = s.default_library;
        let mut a = article("A");
        a.tags = vec![ItemTag::manual("llm"), ItemTag::manual("survey")];
        s.items.create(lib, a).await.unwrap();
        let mut b = article("B");
        b.tags = vec![ItemTag::manual("llm")];
        s.items.create(lib, b).await.unwrap();

        let both = s
            .items
            .count(&ItemFilter { library_id: lib, tags: vec!["llm".into()], ..Default::default() })
            .await
            .unwrap();
        assert_eq!(both, 2);

        let excluded = s
            .items
            .count(&ItemFilter {
                library_id: lib,
                tags: vec!["llm".into(), "-survey".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(excluded, 1);

        let tags = s.tags.list(lib, None, 10).await.unwrap();
        assert_eq!(tags[0].name, "llm");
        assert_eq!(tags[0].count, 2);
    }

    #[tokio::test]
    async fn tag_rename_merges_into_existing() {
        let s = store();
        let lib = s.default_library;
        let mut a = article("A");
        a.tags = vec![ItemTag::manual("ml")];
        s.items.create(lib, a).await.unwrap();
        let mut b = article("B");
        b.tags = vec![ItemTag::manual("machine learning")];
        s.items.create(lib, b).await.unwrap();

        s.tags.rename(lib, "ml", "machine learning").await.unwrap();
        let tags = s.tags.list(lib, None, 10).await.unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].count, 2);
    }

    #[tokio::test]
    async fn every_tag_mutation_bumps_the_library_version() {
        let s = store();
        let lib = s.default_library;
        let mut a = article("A");
        a.tags = vec![ItemTag::manual("ml")];
        s.items.create(lib, a).await.unwrap();

        // Colour changes must be visible to sync clients and read caches.
        let before = s.libraries.version(lib).await.unwrap();
        s.tags.set_color(lib, "ml", Some("#ff0000")).await.unwrap();
        let after = s.libraries.version(lib).await.unwrap();
        assert!(after > before, "set_color must bump the version");

        s.tags.rename(lib, "ml", "machine learning").await.unwrap();
        assert!(s.libraries.version(lib).await.unwrap() > after);
    }

    #[tokio::test]
    async fn tag_counts_refresh_after_a_write() {
        let s = store();
        let lib = s.default_library;
        let mut a = article("A");
        a.tags = vec![ItemTag::manual("ml")];
        s.items.create(lib, a).await.unwrap();
        assert_eq!(s.tags.list(lib, None, 10).await.unwrap()[0].count, 1);

        let mut b = article("B");
        b.tags = vec![ItemTag::manual("ml")];
        s.items.create(lib, b).await.unwrap();
        // Would fail if the cache served the previous generation.
        assert_eq!(s.tags.list(lib, None, 10).await.unwrap()[0].count, 2);
    }

    #[tokio::test]
    async fn sorting_is_stable_and_correct() {
        let s = store();
        let lib = s.default_library;
        for t in ["Charlie", "alpha", "Bravo"] {
            s.items.create(lib, article(t)).await.unwrap();
        }
        let page = s
            .items
            .list(&ItemQuery {
                filter: ItemFilter { library_id: lib, ..Default::default() },
                sort: SortField::Title,
                direction: Direction::Asc,
                ..Default::default()
            })
            .await
            .unwrap();
        let titles: Vec<&str> = page.items.iter().map(|i| i.title()).collect();
        assert_eq!(titles, vec!["alpha", "Bravo", "Charlie"]);
    }

    #[tokio::test]
    async fn duplicate_detection_by_doi() {
        let s = store();
        let lib = s.default_library;
        let created =
            s.items.create(lib, article("A").with_field("DOI", "10.1000/xyz")).await.unwrap();
        let hits = s.items.find_by_fingerprint(lib, &[created.fingerprint()]).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn permanent_delete_writes_tombstone() {
        let s = store();
        let lib = s.default_library;
        let item = s.items.create(lib, article("A")).await.unwrap();
        assert_eq!(s.items.delete(lib, std::slice::from_ref(&item.key)).await.unwrap(), 1);
        assert!(s.items.get(lib, &item.key).await.is_err());
        let n: i64 = s
            .db()
            .call(|c| c.query_row("SELECT count(*) FROM deletions", [], |r| r.get(0)).map_err(sql_err))
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn children_are_returned_for_parents() {
        let s = store();
        let lib = s.default_library;
        let parent = s.items.create(lib, article("Paper")).await.unwrap();
        let mut note = ItemDraft::new("note");
        note.parent_key = Some(parent.key.clone());
        note.fields.insert("note".into(), "my thoughts".into());
        s.items.create(lib, note).await.unwrap();

        let kids = s.items.children(lib, &parent.key).await.unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].item_type, "note");
    }

    #[tokio::test]
    async fn conversations_keep_history_in_order() {
        let s = store();
        let lib = s.default_library;
        let convo = s.conversations.create(lib, "扩散模型", Some("collection:ABC")).await.unwrap();
        assert_eq!(convo.message_count, 0);

        for (role, content) in [("user", "找几篇综述"), ("assistant", "找到 3 篇")] {
            s.conversations
                .append(
                    lib,
                    &convo.key,
                    MessageDraft { role: role.into(), content: content.into(), meta: None, mentions: Vec::new() },
                )
                .await
                .unwrap();
        }

        let messages = s.conversations.messages(lib, &convo.key).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user", "insertion order is preserved");
        assert_eq!(s.conversations.get(lib, &convo.key).await.unwrap().message_count, 2);
    }

    #[tokio::test]
    async fn a_paper_can_be_asked_what_was_said_about_it() {
        let s = store();
        let lib = s.default_library;
        let paper = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let other = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();

        let convo = s.conversations.create(lib, "about it", None).await.unwrap();
        s.conversations
            .append(
                lib,
                &convo.key,
                MessageDraft {
                    role: "user".into(),
                    content: "what does this argue?".into(),
                    meta: None,
                    mentions: vec![paper.key.clone()],
                },
            )
            .await
            .unwrap();

        let found = s.conversations.mentioning(lib, &paper.key).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, convo.key);

        // And a paper nobody asked about has no history, rather than
        // inheriting the conversation's.
        assert!(s.conversations.mentioning(lib, &other.key).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_conversation_that_names_a_paper_twice_is_still_one_conversation() {
        let s = store();
        let lib = s.default_library;
        let paper = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let convo = s.conversations.create(lib, "repeat", None).await.unwrap();

        for _ in 0..3 {
            s.conversations
                .append(
                    lib,
                    &convo.key,
                    MessageDraft {
                        role: "user".into(),
                        content: "again".into(),
                        meta: None,
                        mentions: vec![paper.key.clone()],
                    },
                )
                .await
                .unwrap();
        }

        assert_eq!(s.conversations.mentioning(lib, &paper.key).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn mentions_come_back_with_the_thread() {
        let s = store();
        let lib = s.default_library;
        let paper = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let convo = s.conversations.create(lib, "t", None).await.unwrap();

        s.conversations
            .append(
                lib,
                &convo.key,
                MessageDraft {
                    role: "user".into(),
                    content: "about @this".into(),
                    meta: None,
                    mentions: vec![paper.key.clone()],
                },
            )
            .await
            .unwrap();

        // The client renders the mention as a chip, so reading a thread has to
        // return what was named — not just record it for the reverse lookup.
        let messages = s.conversations.messages(lib, &convo.key).await.unwrap();
        assert_eq!(messages[0].mentions, vec![paper.key]);
    }

    #[tokio::test]
    async fn conversations_sort_by_recency() {
        let s = store();
        let lib = s.default_library;
        let older = s.conversations.create(lib, "older", None).await.unwrap();
        let newer = s.conversations.create(lib, "newer", None).await.unwrap();
        // Appending to the older thread must float it back to the top.
        s.conversations
            .append(
                lib,
                &older.key,
                MessageDraft { role: "user".into(), content: "ping".into(), meta: None, mentions: Vec::new() },
            )
            .await
            .unwrap();

        let listed = s.conversations.list(lib, 10).await.unwrap();
        assert_eq!(listed[0].key, older.key);
        assert_eq!(listed[1].key, newer.key);
    }

    #[tokio::test]
    async fn deleting_a_conversation_takes_its_messages() {
        let s = store();
        let lib = s.default_library;
        let convo = s.conversations.create(lib, "temp", None).await.unwrap();
        s.conversations
            .append(
                lib,
                &convo.key,
                MessageDraft { role: "user".into(), content: "x".into(), meta: None, mentions: Vec::new() },
            )
            .await
            .unwrap();

        assert_eq!(s.conversations.delete(lib, &convo.key).await.unwrap(), 1);
        let orphans: i64 = s
            .db()
            .call(|c| {
                c.query_row("SELECT count(*) FROM messages", [], |r| r.get(0)).map_err(sql_err)
            })
            .await
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[tokio::test]
    async fn rejects_an_unknown_message_role() {
        let s = store();
        let lib = s.default_library;
        let convo = s.conversations.create(lib, "t", None).await.unwrap();
        let err = s
            .conversations
            .append(
                lib,
                &convo.key,
                MessageDraft { role: "hacker".into(), content: "x".into(), meta: None, mentions: Vec::new() },
            )
            .await
            .unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Invalid);
    }

    #[tokio::test]
    async fn smart_collections_round_trip() {
        let s = store();
        let lib = s.default_library;
        let created = s
            .smart
            .create(
                lib,
                SmartCollectionDraft {
                    name: "近年综述".into(),
                    query: "tag:综述 year:2020..2024".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(created.query, "tag:综述 year:2020..2024");
        assert_eq!(created.mode, "hybrid", "defaults are applied");

        let listed = s.smart.list(lib).await.unwrap();
        assert_eq!(listed.len(), 1);

        let updated = s
            .smart
            .update(
                lib,
                &created.key,
                SmartCollectionPatch { name: Some("综述".into()), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "综述");
        assert_eq!(updated.query, created.query, "untouched fields survive");
        assert!(updated.version > created.version);

        assert_eq!(s.smart.delete(lib, &created.key).await.unwrap(), 1);
        assert!(s.smart.list(lib).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn smart_collection_requires_a_name() {
        let s = store();
        let err = s
            .smart
            .create(s.default_library, SmartCollectionDraft { name: "  ".into(), ..Default::default() })
            .await
            .unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Invalid);
    }

    #[tokio::test]
    async fn missing_smart_collection_is_not_found() {
        let s = store();
        let err = s.smart.get(s.default_library, &yk_core::Key::generate()).await.unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let s = store();
        s.settings.set("ui.theme", &serde_json::json!("dark")).await.unwrap();
        assert_eq!(s.settings.get("ui.theme").await.unwrap(), Some(serde_json::json!("dark")));
        assert_eq!(s.settings.list("ui.").await.unwrap().len(), 1);
        s.settings.delete("ui.theme").await.unwrap();
        assert!(s.settings.get("ui.theme").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn since_returns_only_changed_objects() {
        let s = store();
        let lib = s.default_library;
        s.items.create(lib, article("old")).await.unwrap();
        let watermark = s.libraries.version(lib).await.unwrap();
        s.items.create(lib, article("new")).await.unwrap();

        let delta = s
            .items
            .list(&ItemQuery {
                filter: ItemFilter { library_id: lib, since: Some(watermark), ..Default::default() },
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(delta.total, 1);
        assert_eq!(delta.items[0].title(), "new");
    }

    #[tokio::test]
    async fn collections_carry_a_colour_and_an_icon() {
        let s = store();
        let lib = s.default_library;
        let made = s
            .collections
            .create(
                lib,
                CollectionDraft {
                    name: "Reading".into(),
                    color: Some("amber".into()),
                    icon: Some("book".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(made.color.as_deref(), Some("amber"));

        let listed = &s.collections.list(lib).await.unwrap()[0];
        assert_eq!(listed.icon.as_deref(), Some("book"), "appearance survives a round trip");

        // Clearing is distinct from leaving alone, which is the whole reason
        // the patch field is a nested option.
        let cleared = s
            .collections
            .update(
                lib,
                &made.key,
                CollectionPatch { color: Some(None), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(cleared.color, None);
        assert_eq!(cleared.icon.as_deref(), Some("book"), "an absent field is untouched");
    }

    #[tokio::test]
    async fn embed_queue_is_populated_for_regular_items() {
        let s = store();
        s.items.create(s.default_library, article("A")).await.unwrap();
        let n: i64 = s
            .db()
            .call(|c| {
                c.query_row("SELECT count(*) FROM embed_queue", [], |r| r.get(0)).map_err(sql_err)
            })
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::*;
    use yk_core::model::{ItemDraft, ItemTag};

    #[tokio::test]
    async fn attachments_come_back_without_their_tags_and_shelves() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let mut parent = ItemDraft::new("journalArticle").with_field("title", "A paper");
        parent.tags = vec![ItemTag { tag: "keep".into(), r#type: 0 }];
        let parent = s.items.create(lib, parent).await.unwrap();

        let mut file = ItemDraft::new("attachment").with_field("filename", "paper.pdf");
        file.parent_key = Some(parent.key.clone());
        file.tags = vec![ItemTag { tag: "scanned".into(), r#type: 0 }];
        s.items.create(lib, file).await.unwrap();

        let page = s.items.attachments(lib, 100, 0).await.unwrap();
        let (attachment, found_parent) = &page.items[0];

        // Deliberately absent, and asserted so nobody adds it back without
        // meaning to. Nothing that lists files wants an attachment's tags: the
        // browser shows the name, the parent, the address and the size, and
        // renaming wants the parent's title, creators and year — creators
        // travel in the row itself.
        //
        // Loading them cost most of a rename preview, and did it through an
        // `IN (…)` of one placeholder per attachment. SQLite allows 32766 of
        // them, so at thirty thousand files this was not slow, it was a
        // hundred and sixty short of failing outright.
        assert!(attachment.tags.is_empty(), "attachments are listed, not inspected");
        assert!(attachment.collections.is_empty());

        // What the caller actually needs is all there.
        assert_eq!(attachment.field("filename"), Some("paper.pdf"));
        assert_eq!(found_parent.as_ref().map(|p| p.title()), Some("A paper"));
    }

    /// How many keys a "select all, then trash" sends on a large library.
    ///
    /// SQLite allows 32766 bound variables. Anything that builds one
    /// placeholder per key therefore has a ceiling, and the ceiling is not a
    /// slowdown — it is an error on an operation that has always worked, met
    /// only by whoever's library is the first to be big enough.
    const OVER_SQLITE_LIMIT: usize = 40_000;

    #[tokio::test]
    async fn trashing_more_items_than_sqlite_has_variables() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let drafts: Vec<ItemDraft> = (0..OVER_SQLITE_LIMIT)
            .map(|i| ItemDraft::new("journalArticle").with_field("title", format!("Paper {i}")))
            .collect();
        let created = s.items.create_many(lib, drafts).await.unwrap();
        let keys: Vec<yk_core::Key> =
            created.into_iter().filter_map(|r| r.ok()).map(|i| i.key).collect();
        assert_eq!(keys.len(), OVER_SQLITE_LIMIT);

        // Selecting everything and trashing it is one gesture in the workbench.
        let trashed = s.items.set_trashed(lib, &keys, true).await.unwrap();
        assert_eq!(trashed as usize, OVER_SQLITE_LIMIT);

        // And it is one transaction: every one of them, or none.
        let q = yk_core::query::ItemQuery {
            filter: yk_core::query::ItemFilter {
                library_id: lib,
                trash: yk_core::query::TrashScope::Only,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(s.items.list(&q).await.unwrap().total as usize, OVER_SQLITE_LIMIT);
    }

    #[tokio::test]
    async fn reading_more_items_than_sqlite_has_variables() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let drafts: Vec<ItemDraft> = (0..OVER_SQLITE_LIMIT)
            .map(|i| ItemDraft::new("journalArticle").with_field("title", format!("Paper {i}")))
            .collect();
        let created = s.items.create_many(lib, drafts).await.unwrap();
        let keys: Vec<yk_core::Key> =
            created.into_iter().filter_map(|r| r.ok()).map(|i| i.key).collect();

        // `get_many` hydrates tags and collections with one placeholder per
        // row, so the shared helper has the same ceiling as the writes did.
        let got = s.items.get_many(lib, &keys).await.unwrap();
        assert_eq!(got.len(), OVER_SQLITE_LIMIT);
    }

    #[tokio::test]
    async fn listing_more_attachments_than_sqlite_has_variables() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let parent = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let drafts: Vec<ItemDraft> = (0..OVER_SQLITE_LIMIT)
            .map(|i| {
                let mut d = ItemDraft::new("attachment").with_field("filename", format!("{i}.pdf"));
                d.parent_key = Some(parent.key.clone());
                d
            })
            .collect();
        s.items.create_many(lib, drafts).await.unwrap();

        // The rename preview asks for every attachment at once, and each one
        // contributes a placeholder to the parent lookup.
        let page = s.items.attachments(lib, u32::MAX, 0).await.unwrap();
        assert_eq!(page.items.len(), OVER_SQLITE_LIMIT);
        assert!(page.items.iter().all(|(_, p)| p.is_some()));
    }

    #[tokio::test]
    async fn an_attachment_whose_parent_is_gone_is_still_listed() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let mut orphan = ItemDraft::new("attachment").with_field("filename", "lost.pdf");
        orphan.parent_key = None;
        s.items.create(lib, orphan).await.unwrap();

        // A file with nothing to belong to is exactly what a file browser is
        // for finding; dropping it from the list would hide the problem.
        let page = s.items.attachments(lib, 100, 0).await.unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].1.is_none());
    }
}
