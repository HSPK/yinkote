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

#[tokio::test]
async fn count_and_list_never_disagree() {
    // They are the same question asked two ways, and for a while they were two
    // implementations — only one of which read the cache. A test that asks both
    // across a write is what keeps them one.
    let (s, lib) = store().await;
    for i in 0..5 {
        let mut draft = article(&format!("Paper {i}"));
        if i % 2 == 0 {
            draft.tags = vec![ItemTag::manual("even")];
        }
        s.items.create(lib, draft).await.unwrap();
    }

    for filter in [all(lib), tagged(lib, "even"), tagged(lib, "nothing")] {
        let listed = total(&s, filter.clone()).await;
        let counted = s.items.count(&filter).await.unwrap();
        assert_eq!(listed, counted, "list and count disagreed for {filter:?}");
    }

    // And again after a write, so a cached answer cannot outlive its state in
    // one of them but not the other.
    s.items.create(lib, article("Late arrival")).await.unwrap();
    assert_eq!(total(&s, all(lib)).await, s.items.count(&all(lib)).await.unwrap());
    assert_eq!(s.items.count(&all(lib)).await.unwrap(), 6);
}

#[tokio::test]
async fn the_graph_sees_the_same_library_size_as_everything_else() {
    // The graph excludes tags that are too common to mean anything, and the
    // threshold is a share of how many live items there are — read through the
    // same cache as every other count. A stale total would leave the graph
    // judging "too common" by yesterday's library.
    //
    // So: a tag on every item is a useful link in a small library and noise in
    // a large one, and the switch is the observable proof that the ceiling saw
    // the library grow.
    let (s, lib) = store().await;
    let mut first = None;
    for i in 0..40 {
        let mut draft = article(&format!("Paper {i}"));
        draft.tags = vec![ItemTag::manual("shared")];
        let created = s.items.create(lib, draft).await.unwrap();
        first.get_or_insert(created.key);
    }
    let first = first.unwrap();
    assert_eq!(s.items.count(&all(lib)).await.unwrap(), 40);
    assert!(
        !s.graph.neighbours(lib, &first, 8).await.unwrap().is_empty(),
        "under the floor, a shared tag still links things"
    );

    for i in 0..400 {
        let mut draft = article(&format!("Later {i}"));
        draft.tags = vec![ItemTag::manual("shared")];
        s.items.create(lib, draft).await.unwrap();
    }
    assert_eq!(s.items.count(&all(lib)).await.unwrap(), 440);

    assert!(
        s.graph.neighbours(lib, &first, 8).await.unwrap().is_empty(),
        "a tag on the whole library is not a reason to draw an edge — and the \
         ceiling could only know that by reading the new total"
    );
}

#[tokio::test]
async fn the_attachment_total_is_the_same_question_too() {
    // The file browser reports how many files there are; that is the item
    // count with one more clause, and it was the fourth place to write the SQL
    // by hand. It has to move with the library like every other count.
    let (s, lib) = store().await;
    let paper = s.items.create(lib, article("Paper")).await.unwrap();

    let files = || async {
        s.items.attachments(lib, 10, 0).await.unwrap().total
    };
    assert_eq!(files().await, 0);

    let mut att = ItemDraft::new("attachment").with_field("filename", "a.pdf");
    att.parent_key = Some(paper.key.clone());
    let created = s.items.create(lib, att).await.unwrap();
    assert_eq!(files().await, 1, "a new file is counted");

    s.items.set_trashed(lib, std::slice::from_ref(&created.key), true).await.unwrap();
    assert_eq!(files().await, 0, "and a trashed one stops being");

    // The paper is not a file, so the two counts are genuinely different
    // questions and must not answer each other.
    assert_eq!(s.items.count(&all(lib)).await.unwrap(), 1);
}

#[tokio::test]
async fn the_sidebar_list_moves_with_the_library() {
    // Each row carries how many live items the collection holds, so this list
    // is invalidated by item writes and not only by collection writes — which
    // is the whole reason it is keyed on the library version rather than on
    // anything about collections.
    let (s, lib) = store().await;
    let shelf = s
        .collections
        .create(lib, yk_core::model::CollectionDraft { name: "Shelf".into(), ..Default::default() })
        .await
        .unwrap();

    let held = || async {
        s.collections.list(lib).await.unwrap().into_iter().find(|c| c.key == shelf.key).unwrap()
    };
    assert_eq!(held().await.item_count, 0);

    let item = s.items.create(lib, article("One")).await.unwrap();
    s.items.add_to_collection(lib, &shelf.key, std::slice::from_ref(&item.key)).await.unwrap();
    assert_eq!(held().await.item_count, 1, "joining a shelf shows up in the list");

    // Trashing does not remove the membership, so this only moves if the count
    // really re-runs its `deleted = 0` check.
    s.items.set_trashed(lib, std::slice::from_ref(&item.key), true).await.unwrap();
    assert_eq!(held().await.item_count, 0, "a trashed item stops counting");

    // And renaming a collection is a collection write, which also has to
    // retire the list.
    s.collections
        .update(
            lib,
            &shelf.key,
            yk_core::model::CollectionPatch { name: Some("Renamed".into()), ..Default::default() },
        )
        .await
        .unwrap();
    assert_eq!(held().await.name, "Renamed");
}

#[tokio::test]
async fn the_duplicate_scan_is_redone_after_a_merge() {
    // Two full-library GROUP BYs, so the answer is remembered — which makes
    // "does it notice the library changed" the only question worth asking.
    let (s, lib) = store().await;
    for _ in 0..2 {
        s.items
            .create(
                lib,
                ItemDraft::new("journalArticle")
                    .with_field("title", "The same paper twice")
                    .with_field("date", "2021"),
            )
            .await
            .unwrap();
    }

    let groups = s.items.duplicate_groups(lib, 50).await.unwrap();
    assert_eq!(groups.len(), 1, "two records of one paper is one group");
    assert_eq!(groups[0].len(), 2);
    // Asked twice, answered the same — this is the read the cache serves.
    assert_eq!(s.items.duplicate_groups(lib, 50).await.unwrap().len(), 1);

    let master = groups[0][0].key.clone();
    let other = groups[0][1].key.clone();
    s.items.merge(lib, &master, std::slice::from_ref(&other)).await.unwrap();

    assert!(
        s.items.duplicate_groups(lib, 50).await.unwrap().is_empty(),
        "the pair was merged, so there is nothing left to report"
    );
}

#[tokio::test]
async fn a_different_limit_is_a_different_question() {
    // The scan takes a limit, so two callers asking for different numbers of
    // groups must not answer each other.
    let (s, lib) = store().await;
    for i in 0..6 {
        for _ in 0..2 {
            s.items
                .create(
                    lib,
                    ItemDraft::new("journalArticle")
                        .with_field("title", format!("Paper {i}"))
                        .with_field("date", "2021"),
                )
                .await
                .unwrap();
        }
    }
    assert_eq!(s.items.duplicate_groups(lib, 2).await.unwrap().len(), 2);
    assert_eq!(s.items.duplicate_groups(lib, 6).await.unwrap().len(), 6);
    assert_eq!(s.items.duplicate_groups(lib, 2).await.unwrap().len(), 2, "and back again");
}

#[tokio::test]
async fn the_sidebar_list_catches_up_after_being_served_stale() {
    // The collection list is expensive enough to be handed back one version
    // behind while a fresh one is computed. That is only acceptable if the
    // fresh one actually arrives, so this is the test that matters: read,
    // write, read (may be behind), and it must settle on the truth.
    let (s, lib) = store().await;
    let shelf = s
        .collections
        .create(lib, yk_core::model::CollectionDraft { name: "Shelf".into(), ..Default::default() })
        .await
        .unwrap();
    let held = |s: &Store| {
        let key = shelf.key.clone();
        let s = s.clone();
        async move {
            s.collections.list(lib).await.unwrap().into_iter().find(|c| c.key == key).unwrap()
        }
    };
    assert_eq!(held(&s).await.item_count, 0);

    let item = s.items.create(lib, article("One")).await.unwrap();
    s.items.add_to_collection(lib, &shelf.key, std::slice::from_ref(&item.key)).await.unwrap();

    // On a library this small the list costs well under a millisecond, so it
    // is recomputed rather than deferred — being exact is free here, which is
    // the point of the cost threshold.
    assert_eq!(held(&s).await.item_count, 1);

    // And it keeps up across further writes rather than pinning one answer.
    let second = s.items.create(lib, article("Two")).await.unwrap();
    s.items.add_to_collection(lib, &shelf.key, std::slice::from_ref(&second.key)).await.unwrap();
    assert_eq!(held(&s).await.item_count, 2);
}
