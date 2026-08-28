//! The listing total is cached against the library version, so this is the
//! property the whole cache rests on: anything that can change a count moves
//! the version.
//!
//! A miss costs a recount. A stale hit shows the user a number that disagrees
//! with the rows next to it, so these are not tests about speed.

use yk_core::model::{ItemDraft, ItemTag};
use yk_core::model::ItemPatch;

use yk_core::query::{ItemFilter, ItemQuery, TrashScope};
use yk_store::Store;

async fn store() -> (Store, i64) {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    (store, lib)
}

fn article(title: &str) -> ItemDraft {
    ItemDraft::new("journalArticle").with_field("title", title)
}

async fn total(store: &Store, filter: ItemFilter) -> i64 {
    store
        .items
        .list(&ItemQuery { filter, limit: 5, ..Default::default() })
        .await
        .unwrap()
        .total
}

fn all(lib: i64) -> ItemFilter {
    ItemFilter { library_id: lib, ..Default::default() }
}

fn tagged(lib: i64, tag: &str) -> ItemFilter {
    ItemFilter { library_id: lib, tags: vec![tag.into()], ..Default::default() }
}

#[tokio::test]
async fn creating_an_item_changes_the_total_it_is_counted_in() {
    let (s, lib) = store().await;
    assert_eq!(total(&s, all(lib)).await, 0);
    s.items.create(lib, article("One")).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 1, "the cached total survived a write");
    s.items.create(lib, article("Two")).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 2);
}

#[tokio::test]
async fn trashing_and_restoring_move_the_total_both_ways() {
    let (s, lib) = store().await;
    let one = s.items.create(lib, article("One")).await.unwrap();
    s.items.create(lib, article("Two")).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 2);

    s.items.set_trashed(lib, std::slice::from_ref(&one.key), true).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 1, "a trashed item left the default scope");
    assert_eq!(
        total(&s, ItemFilter { trash: TrashScope::Only, ..all(lib) }).await,
        1,
        "and arrived in the trash's own count"
    );

    s.items.set_trashed(lib, &[one.key], false).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 2);
}

#[tokio::test]
async fn a_tag_added_later_changes_that_tags_total() {
    let (s, lib) = store().await;
    let item = s.items.create(lib, article("One")).await.unwrap();
    assert_eq!(total(&s, tagged(lib, "survey")).await, 0);

    // The count that must move is not the one the write obviously touches:
    // editing an item changes what every tag filter would report.
    let patch = ItemPatch { tags: Some(vec![ItemTag::manual("survey")]), ..Default::default() };
    s.items.update(lib, &item.key, patch, None).await.unwrap();

    assert_eq!(total(&s, tagged(lib, "survey")).await, 1);
    assert_eq!(total(&s, all(lib)).await, 1, "and the library total did not double-count");
}

#[tokio::test]
async fn joining_a_collection_changes_that_collections_total() {
    let (s, lib) = store().await;
    let shelf = s
        .collections
        .create(lib, yk_core::model::CollectionDraft { name: "Shelf".into(), ..Default::default() })
        .await
        .unwrap();
    let item = s.items.create(lib, article("One")).await.unwrap();

    let on_shelf =
        || ItemFilter { collection: Some(shelf.key.clone()), ..all(lib) };
    assert_eq!(total(&s, on_shelf()).await, 0);

    s.items.add_to_collection(lib, &shelf.key, &[item.key]).await.unwrap();
    assert_eq!(total(&s, on_shelf()).await, 1, "membership is a count too");
}

#[tokio::test]
async fn permanent_deletion_is_reflected() {
    let (s, lib) = store().await;
    let one = s.items.create(lib, article("One")).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 1);
    s.items.delete(lib, &[one.key]).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, 0);
}
