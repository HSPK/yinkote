//! SQLite persistence adapters.
//!
//! Exposes one concrete implementation per repository port plus a [`Store`]
//! aggregate that owns the connection pool and hands out `Arc`'d ports.

mod cache;
mod collections;
mod db;
pub mod filter;
pub mod index;
mod items;

use std::path::Path;
use std::sync::Arc;

pub use cache::CachingTagRepository;
pub use collections::{SqliteCollectionRepository, SqliteLibraryRepository, SqliteTagRepository};
pub use db::{sql_err, write_tx, Db, Pool, PooledConn};
pub use items::{SqliteItemRepository, SqliteSettingsRepository};

use yk_core::ports::*;
use yk_core::Result;

/// Composition root for persistence. Cloning is cheap.
#[derive(Clone)]
pub struct Store {
    db: Db,
    pub libraries: Arc<dyn LibraryRepository>,
    pub items: Arc<dyn ItemRepository>,
    pub collections: Arc<dyn CollectionRepository>,
    pub tags: Arc<dyn TagRepository>,
    pub settings: Arc<dyn SettingsRepository>,
    /// Concrete handle for operations outside the port surface (index rebuild).
    items_impl: SqliteItemRepository,
    /// Id of the personal library, created on first run.
    pub default_library: i64,
}

impl Store {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let db = Db::open(path)?;
        let default_library = SqliteLibraryRepository::ensure_default(&db, "我的文库")?;
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
