//! SQLite persistence adapters.
//!
//! Exposes one concrete implementation per repository port plus a [`Store`]
//! aggregate that owns the connection pool and hands out `Arc`'d ports.

mod cache;
pub mod counts;
mod collections;
mod conversations;
mod db;
pub mod filter;
pub mod downloads;
pub mod graph;
pub mod relations;
pub mod index;
mod items;

/// The SQL whose *query plan* is part of its correctness.
///
/// Grouped and re-exported so the guard in `tests/fingerprint_plans.rs` is a
/// list rather than a habit. Three separate times, a fingerprint lookup has
/// silently fallen back to `idx_items_year` and scanned the library; the
/// results were right every time, which is why nothing but a plan assertion
/// ever noticed.
pub mod plans {
    pub use crate::graph::COCITATION_SQL;
    pub use crate::items::{fingerprint_sql, DUPLICATE_SCAN_SQL};
    pub use crate::relations::{CITES_SQL, MISSING_SQL};
}
mod smart;

use std::path::Path;
use std::sync::Arc;

pub use cache::CachingTagRepository;
pub use collections::{SqliteCollectionRepository, SqliteLibraryRepository, SqliteTagRepository};
pub use db::{item_count_of, sql_err, write_tx, Db, Pool, PooledConn};
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
        // One cache, shared. Counting live items is asked by the listing, by
        // `/stats` and by the graph's common-tag ceiling; three copies meant
        // three recounts of the same number in the same unchanged library.
        let counts: Arc<crate::counts::CountCache> = Default::default();
        let items_impl = SqliteItemRepository::new(db.clone(), counts.clone());
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
            graph: Arc::new(SqliteGraphRepository::new(db.clone(), counts.clone())),
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
    async fn a_batch_update_is_one_change_at_one_version() {
        let s = store();
        let lib = s.default_library;
        let a = s.items.create(lib, article("A")).await.unwrap();
        let b = s.items.create(lib, article("B")).await.unwrap();
        let before = s.libraries.version(lib).await.unwrap();

        let patch = |title: &str| ItemPatch {
            fields: Some(fields(serde_json::json!({ "title": title }))),
            ..Default::default()
        };
        let out = s
            .items
            .update_many(lib, vec![(a.key.clone(), patch("A2")), (b.key.clone(), patch("B2"))])
            .await
            .unwrap();

        assert_eq!(out.len(), 2);
        let versions: Vec<i64> = out.iter().map(|r| r.as_ref().unwrap().version).collect();
        assert_eq!(versions[0], versions[1], "one batch, one version");
        assert_eq!(
            s.libraries.version(lib).await.unwrap(),
            before + 1,
            "renaming a library's files must not leave a version per file behind"
        );
        assert_eq!(s.items.get(lib, &a.key).await.unwrap().title(), "A2");
        assert_eq!(s.items.get(lib, &b.key).await.unwrap().title(), "B2");
    }

    #[tokio::test]
    async fn one_bad_row_does_not_take_the_batch_with_it() {
        let s = store();
        let lib = s.default_library;
        let good = s.items.create(lib, article("A")).await.unwrap();
        let gone = yk_core::Key::generate();

        let patch = || ItemPatch {
            fields: Some(fields(serde_json::json!({ "title": "renamed" }))),
            ..Default::default()
        };
        let out = s
            .items
            .update_many(lib, vec![(gone, patch()), (good.key.clone(), patch())])
            .await
            .unwrap();

        assert!(out[0].is_err(), "an item that is no longer there cannot be patched");
        assert!(out[1].is_ok(), "and that must not cost the item that is");
        assert_eq!(s.items.get(lib, &good.key).await.unwrap().title(), "renamed");
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

    /// A paper's notes and files follow it into the trash, and back out.
    ///
    /// They used not to. The trash listed the paper alone while its note still
    /// answered a search as a live result, and emptying the trash then
    /// destroyed that note through `items.parent_id ON DELETE CASCADE` —
    /// content the user was never shown as being on its way out.
    #[tokio::test]
    async fn a_papers_notes_and_files_go_to_the_trash_with_it() {
        let s = store();
        let lib = s.default_library;
        let paper = s.items.create(lib, article("Kept work")).await.unwrap();

        let mut note = ItemDraft::new("note").with_field("note", "Worth keeping.");
        note.parent_key = Some(paper.key.clone());
        let note = s.items.create(lib, note).await.unwrap();

        let mut pdf = ItemDraft::new("attachment").with_field("filename", "paper.pdf");
        pdf.parent_key = Some(paper.key.clone());
        let pdf = s.items.create(lib, pdf).await.unwrap();

        s.items.set_trashed(lib, std::slice::from_ref(&paper.key), true).await.unwrap();

        for key in [&note.key, &pdf.key] {
            let child = s.items.get(lib, key).await.unwrap();
            assert!(child.deleted, "a child was left live under a trashed paper");
        }

        // And out again, or restoring a paper would return it without its PDF.
        s.items.set_trashed(lib, std::slice::from_ref(&paper.key), false).await.unwrap();
        for key in [&paper.key, &note.key, &pdf.key] {
            assert!(!s.items.get(lib, key).await.unwrap().deleted, "restore left something behind");
        }
    }

    /// Trashing a child on its own must not drag the paper down with it.
    #[tokio::test]
    async fn trashing_a_file_leaves_its_paper_alone() {
        let s = store();
        let lib = s.default_library;
        let paper = s.items.create(lib, article("Still here")).await.unwrap();
        let mut pdf = ItemDraft::new("attachment").with_field("filename", "paper.pdf");
        pdf.parent_key = Some(paper.key.clone());
        let pdf = s.items.create(lib, pdf).await.unwrap();

        s.items.set_trashed(lib, std::slice::from_ref(&pdf.key), true).await.unwrap();

        assert!(s.items.get(lib, &pdf.key).await.unwrap().deleted);
        assert!(!s.items.get(lib, &paper.key).await.unwrap().deleted, "the paper went too");
    }

    /// Deleting a shelf must never delete what is on it.
    ///
    /// Untested until now, which is uncomfortable for the one operation here
    /// that a user would experience as losing their library. It works because
    /// `collection_items` cascades on the *membership* row and `collections`
    /// cascades on the parent — two schema declarations and a
    /// `PRAGMA foreign_keys = ON`, none of which the code mentions and any of
    /// which could be changed by someone with no idea this depended on them.
    #[tokio::test]
    async fn deleting_a_shelf_keeps_the_papers_on_it() {
        let s = store();
        let lib = s.default_library;
        let shelf = s
            .collections
            .create(lib, CollectionDraft { name: "Doomed".into(), ..Default::default() })
            .await
            .unwrap();

        let mut draft = article("A paper on a doomed shelf");
        draft.collections = vec![shelf.key.clone()];
        let paper = s.items.create(lib, draft).await.unwrap();

        s.collections.delete(lib, &shelf.key, false).await.unwrap();

        let kept = s.items.get(lib, &paper.key).await.unwrap();
        assert!(!kept.deleted, "the paper went with the shelf");
        assert!(kept.collections.is_empty(), "it is still filed on a shelf that is gone");
    }

    /// A shelf inside a deleted one is moved up, not stranded.
    ///
    /// Stranding it would leave a shelf whose parent does not exist: it draws
    /// nowhere in a tree, so the shelf and everything filed on it become
    /// unreachable without ever being deleted.
    #[tokio::test]
    async fn a_child_shelf_is_promoted_rather_than_stranded() {
        let s = store();
        let lib = s.default_library;
        let parent = s
            .collections
            .create(lib, CollectionDraft { name: "Parent".into(), ..Default::default() })
            .await
            .unwrap();
        let child = s
            .collections
            .create(
                lib,
                CollectionDraft {
                    name: "Child".into(),
                    parent_key: Some(parent.key.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        s.collections.delete(lib, &parent.key, false).await.unwrap();

        let all = s.collections.list(lib).await.unwrap();
        let moved = all.iter().find(|c| c.key == child.key).expect("the child went too");
        assert_eq!(moved.parent_key, None, "the child was left pointing at a shelf that is gone");
    }

    /// Recursive is allowed to take the shelves. It is still not allowed to
    /// take the papers.
    #[tokio::test]
    async fn a_recursive_delete_takes_the_shelves_and_leaves_the_papers() {
        let s = store();
        let lib = s.default_library;
        let parent = s
            .collections
            .create(lib, CollectionDraft { name: "Top".into(), ..Default::default() })
            .await
            .unwrap();
        let child = s
            .collections
            .create(
                lib,
                CollectionDraft {
                    name: "Under".into(),
                    parent_key: Some(parent.key.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut draft = article("A paper two levels down");
        draft.collections = vec![child.key.clone()];
        let paper = s.items.create(lib, draft).await.unwrap();

        s.collections.delete(lib, &parent.key, true).await.unwrap();

        let all = s.collections.list(lib).await.unwrap();
        assert!(!all.iter().any(|c| c.key == child.key), "the subtree survived a recursive delete");
        // And nothing is left pointing at a shelf that no longer exists.
        let keys: Vec<_> = all.iter().map(|c| c.key.clone()).collect();
        assert!(
            all.iter().all(|c| c.parent_key.as_ref().is_none_or(|p| keys.contains(p))),
            "a shelf was stranded",
        );

        let kept = s.items.get(lib, &paper.key).await.unwrap();
        assert!(!kept.deleted, "a recursive shelf delete took a paper with it");
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
    async fn a_patch_changes_only_what_it_names() {
        use yk_core::model::ConversationPatch;

        let s = store();
        let lib = s.default_library;
        let convo = s.conversations.create(lib, "Diffusion", Some("COLL1234")).await.unwrap();

        // Renaming must not detach the conversation from its collection.
        let renamed = s
            .conversations
            .update(
                lib,
                &convo.key,
                ConversationPatch { title: Some("Diffusion models".into()), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(renamed.title, "Diffusion models");
        assert_eq!(renamed.scope.as_deref(), Some("COLL1234"));

        // Scoping must not rewrite the title.
        let scoped = s
            .conversations
            .update(
                lib,
                &convo.key,
                ConversationPatch { scope: Some(Some("OTHER456".into())), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(scoped.title, "Diffusion models");
        assert_eq!(scoped.scope.as_deref(), Some("OTHER456"));

        // Null clears it — which is a different request from not mentioning it.
        let cleared = s
            .conversations
            .update(lib, &convo.key, ConversationPatch { scope: Some(None), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(cleared.scope, None);
        assert_eq!(cleared.title, "Diffusion models");
    }

    #[tokio::test]
    async fn an_empty_scope_is_the_same_as_none() {
        use yk_core::model::ConversationPatch;

        let s = store();
        let lib = s.default_library;
        let convo = s.conversations.create(lib, "t", Some("COLL1234")).await.unwrap();

        // Otherwise the same state would have two representations, and every
        // check for "is this scoped" would have to know about both.
        let updated = s
            .conversations
            .update(
                lib,
                &convo.key,
                ConversationPatch { scope: Some(Some("  ".into())), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(updated.scope, None);
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
    async fn reading_the_children_of_more_parents_than_sqlite_has_variables() {
        // "Select all, empty the trash" passes the whole library through here,
        // and a statement binds one value per key. This is the same ceiling
        // that made trashing a large selection fail outright.
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let parents: Vec<ItemDraft> =
            (0..OVER_SQLITE_LIMIT).map(|_| ItemDraft::new("journalArticle")).collect();
        let made = s.items.create_many(lib, parents).await.unwrap();
        let keys: Vec<yk_core::Key> =
            made.into_iter().filter_map(|r| r.ok()).map(|i| i.key).collect();

        // One child, on the last parent, so a chunking bug that drops a run
        // shows up as an empty answer rather than a smaller one.
        let mut child = ItemDraft::new("attachment").with_field("filename", "x.pdf");
        child.parent_key = Some(keys.last().unwrap().clone());
        s.items.create(lib, child).await.unwrap();

        let found = s.items.children_of(lib, &keys).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].field("filename"), Some("x.pdf"));
    }

    #[tokio::test]
    async fn the_children_of_many_parents_come_back_together() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let a = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let b = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let lonely = s.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();

        for (parent, name) in [(&a.key, "a.pdf"), (&a.key, "a2.pdf"), (&b.key, "b.pdf")] {
            let mut child = ItemDraft::new("attachment").with_field("filename", name);
            child.parent_key = Some(parent.clone());
            s.items.create(lib, child).await.unwrap();
        }

        let found = s.items.children_of(lib, &[a.key.clone(), b.key.clone()]).await.unwrap();
        assert_eq!(found.len(), 3, "every child of both parents, in one answer");

        // A parent with nothing under it contributes nothing rather than
        // erroring, which is what a deletion of mixed items looks like.
        let none = s.items.children_of(lib, &[lonely.key]).await.unwrap();
        assert!(none.is_empty());
        assert!(s.items.children_of(lib, &[]).await.unwrap().is_empty());
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

#[cfg(test)]
mod counting_tests {
    use super::*;
    use yk_core::model::{CollectionDraft, ItemDraft, ItemTag};

    /// Counting must not be a listing with `.len()` on it.
    ///
    /// The statistics endpoint answered "how many tags" by grouping over every
    /// `item_tags` row — 303ms on a hundred-thousand-item library to learn the
    /// number 33 — and "how many collections" by attaching a membership count
    /// to each one. Both questions are row counts.
    #[tokio::test]
    async fn counts_tags_and_collections_without_aggregating_their_contents() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        for name in ["survey", "diffusion", "attention"] {
            let mut draft = ItemDraft::new("journalArticle");
            draft.tags = vec![ItemTag { tag: name.into(), r#type: 0 }];
            s.items.create(lib, draft).await.unwrap();
        }
        for name in ["To read", "Done"] {
            s.collections
                .create(lib, CollectionDraft { name: name.into(), ..Default::default() })
                .await
                .unwrap();
        }

        assert_eq!(s.tags.count(lib).await.unwrap(), 3);
        assert_eq!(s.collections.count(lib).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn a_tag_on_nothing_still_counts_as_a_tag() {
        let s = Store::in_memory().unwrap();
        let lib = s.default_library;

        let mut draft = ItemDraft::new("journalArticle");
        draft.tags = vec![ItemTag { tag: "orphan".into(), r#type: 0 }];
        let item = s.items.create(lib, draft).await.unwrap();
        s.items.delete(lib, &[item.key]).await.unwrap();

        // The row survives its last use, and the count describes the rows.
        // Saying so here because the *listing* filters these out, and the two
        // numbers are allowed to differ.
        assert_eq!(s.tags.count(lib).await.unwrap(), 1);
    }

    /// What `create` answers must be what the library holds.
    ///
    /// Tags were trimmed inside `set_tags`, which writes the rows — but the
    /// item handed back was built from the draft and then had the raw list
    /// copied over it. Creating an item tagged `"   "` and `"  Kept  "`
    /// answered with both, spelled as sent, while the library held one tag
    /// called `Kept`. The workbench puts created items straight into the list,
    /// so a tag that does not exist sat on screen until a reload.
    #[tokio::test]
    async fn a_created_item_reports_the_tags_that_were_stored() {
        let store = Store::in_memory().unwrap();
        let lib = store.default_library;

        let mut draft = ItemDraft::new("journalArticle").with_field("title", "Tag shapes");
        draft.tags = vec![
            ItemTag { tag: "   ".into(), r#type: 0 },
            ItemTag { tag: "  Kept  ".into(), r#type: 0 },
            ItemTag { tag: "Kept".into(), r#type: 0 },
        ];
        let created = store.items.create(lib, draft).await.unwrap();

        let names: Vec<&str> = created.tags.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(names, vec!["Kept"], "the reply did not match what was written");

        let stored = store.items.get(lib, &created.key).await.unwrap();
        let stored_names: Vec<&str> = stored.tags.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(stored_names, names, "reply and storage disagree");
    }

    /// The same on the way through a patch, which is the other way a raw tag
    /// reaches the store.
    #[tokio::test]
    async fn a_patched_item_reports_the_tags_that_were_stored() {
        let store = Store::in_memory().unwrap();
        let lib = store.default_library;
        let item = store
            .items
            .create(lib, ItemDraft::new("journalArticle").with_field("title", "Patch shapes"))
            .await
            .unwrap();

        let patch = yk_core::model::ItemPatch {
            tags: Some(vec![
                ItemTag { tag: " Padded ".into(), r#type: 0 },
                ItemTag { tag: "\t".into(), r#type: 0 },
            ]),
            ..Default::default()
        };
        let updated = store.items.update(lib, &item.key, patch, None).await.unwrap();

        let names: Vec<&str> = updated.tags.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(names, vec!["Padded"]);
        let stored = store.items.get(lib, &item.key).await.unwrap();
        assert_eq!(stored.tags.len(), 1, "reply and storage disagree");
    }
}
