//! A collection's count describes the view its row opens.
//!
//! Browsing a collection includes its sub-collections — `recursive` defaults
//! to true on the items route, and there is a test that says so. The sidebar
//! counted only direct members, so a shelf holding sixty papers with a
//! sub-shelf of sixty more was labelled 60 and listed 120 when clicked.
//!
//! §3.223 fixed the same disagreement between the sidebar and the footer for
//! the library as a whole. Nesting is where it survived, because the smoke
//! check compared a flat count.

use yk_core::model::{CollectionDraft, ItemDraft};
use yk_core::query::ItemQuery;
use yk_store::Store;

/// Both numbers a reader sees for the same shelf: the one on its row, and the
/// one under the list it opens.
async fn counts(store: &Store, lib: i64, key: &yk_core::Key) -> (i64, i64) {
    let listed = store
        .collections
        .list(lib)
        .await
        .unwrap()
        .into_iter()
        .find(|c| &c.key == key)
        .expect("the collection is listed")
        .item_count;

    let opened = store
        .items
        .list(&ItemQuery {
            filter: yk_core::query::ItemFilter {
                library_id: lib,
                collection: Some(key.clone()),
                recursive: true,
                ..Default::default()
            },
            limit: 1,
            ..Default::default()
        })
        .await
        .unwrap()
        .total;

    (listed, opened)
}

async fn shelf(store: &Store, lib: i64, name: &str, parent: Option<yk_core::Key>) -> yk_core::Key {
    let draft = CollectionDraft { name: name.into(), parent_key: parent, ..Default::default() };
    store.collections.create(lib, draft).await.unwrap().key
}

async fn file(store: &Store, lib: i64, collection: &yk_core::Key, titles: &[&str]) {
    for title in titles {
        let item =
            store.items.create(lib, ItemDraft::new("journalArticle").with_field("title", *title))
                .await
                .unwrap();
        store.items.add_to_collection(lib, collection, &[item.key]).await.unwrap();
    }
}

#[tokio::test]
async fn a_parents_count_includes_what_is_under_it() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    let parent = shelf(&store, lib, "Migrated", None).await;
    let child = shelf(&store, lib, "Nested", Some(parent.clone())).await;
    file(&store, lib, &parent, &["Direct one", "Direct two"]).await;
    file(&store, lib, &child, &["Nested one", "Nested two", "Nested three"]).await;

    let (listed, opened) = counts(&store, lib, &parent).await;
    assert_eq!(listed, opened, "the row said {listed} and opening it showed {opened}");
    assert_eq!(listed, 5);

    // And the child still counts only its own.
    let (child_listed, child_opened) = counts(&store, lib, &child).await;
    assert_eq!(child_listed, child_opened);
    assert_eq!(child_listed, 3);
}

/// A paper filed in both a shelf and its sub-shelf is one paper, and the list
/// shows it once.
#[tokio::test]
async fn a_paper_filed_twice_is_counted_once() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    let parent = shelf(&store, lib, "Parent", None).await;
    let child = shelf(&store, lib, "Child", Some(parent.clone())).await;

    let item = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Filed twice"))
        .await
        .unwrap();
    store.items.add_to_collection(lib, &parent, std::slice::from_ref(&item.key)).await.unwrap();
    store.items.add_to_collection(lib, &child, &[item.key]).await.unwrap();

    let (listed, opened) = counts(&store, lib, &parent).await;
    assert_eq!(listed, 1, "counted twice");
    assert_eq!(listed, opened);
}

/// Depth is not one level. A count that joined only the immediate children
/// would pass the first test and fail here.
#[tokio::test]
async fn the_count_reaches_all_the_way_down() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    let top = shelf(&store, lib, "Top", None).await;
    let middle = shelf(&store, lib, "Middle", Some(top.clone())).await;
    let bottom = shelf(&store, lib, "Bottom", Some(middle)).await;
    file(&store, lib, &bottom, &["Deep one", "Deep two"]).await;

    let (listed, opened) = counts(&store, lib, &top).await;
    assert_eq!(listed, 2, "a grandchild's papers are under the top shelf too");
    assert_eq!(listed, opened);
}

/// Trashing a paper takes it out of both numbers, not one.
#[tokio::test]
async fn a_trashed_paper_leaves_both_counts() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    let parent = shelf(&store, lib, "Parent", None).await;
    let child = shelf(&store, lib, "Child", Some(parent.clone())).await;
    file(&store, lib, &child, &["Kept", "Trashed"]).await;

    let doomed = store
        .items
        .list(&ItemQuery {
            filter: yk_core::query::ItemFilter {
                library_id: lib,
                collection: Some(child),
                recursive: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.title() == "Trashed")
        .expect("it is there");
    store.items.set_trashed(lib, &[doomed.key], true).await.unwrap();

    let (listed, opened) = counts(&store, lib, &parent).await;
    assert_eq!(listed, 1);
    assert_eq!(listed, opened);
}
