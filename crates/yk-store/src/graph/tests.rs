//! Tests for the relationship graph.

use crate::Store;
use yk_core::model::{Creator, ItemDraft, ItemTag};

fn tagged(title: &str, tags: &[&str]) -> ItemDraft {
    let mut draft = ItemDraft::new("journalArticle").with_field("title", title);
    draft.tags = tags.iter().map(|t| ItemTag { tag: (*t).into(), r#type: 0 }).collect();
    draft
}

fn by(title: &str, surname: &str) -> ItemDraft {
    ItemDraft::new("journalArticle").with_field("title", title).with_creator(Creator {
        last_name: Some(surname.into()),
        first_name: Some("A".into()),
        ..Default::default()
    })
}

#[tokio::test]
async fn relates_items_that_share_tags_most_shared_first() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let focus = s.items.create(lib, tagged("Focus", &["transformer", "attention"])).await.unwrap();
    s.items.create(lib, tagged("Both", &["transformer", "attention"])).await.unwrap();
    s.items.create(lib, tagged("One", &["transformer"])).await.unwrap();
    s.items.create(lib, tagged("None", &["unrelated"])).await.unwrap();

    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    let titles: Vec<&str> = found.iter().map(|n| n.title.as_str()).collect();

    assert_eq!(titles, vec!["Both", "One"], "sharing two tags beats sharing one");
}

#[tokio::test]
async fn an_edge_says_why_it_exists() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let focus = s.items.create(lib, tagged("Focus", &["transformer"])).await.unwrap();
    s.items.create(lib, tagged("Shares a tag", &["transformer"])).await.unwrap();

    // An unexplained edge is a claim the reader has to take on trust.
    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    assert_eq!(found[0].relation, crate::Relation::Tag);
    assert_eq!(found[0].weight, 1.0);
}

#[tokio::test]
async fn ignores_a_tag_that_is_on_most_of_the_library() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    // `to-read` on everything does not make everything related; it makes it
    // unread. The floor is 50, so this needs to be a big pile.
    for i in 0..60 {
        s.items.create(lib, tagged(&format!("Unread {i}"), &["to-read"])).await.unwrap();
    }
    let focus = s.items.create(lib, tagged("Focus", &["to-read", "rare"])).await.unwrap();
    s.items.create(lib, tagged("Rare too", &["to-read", "rare"])).await.unwrap();

    let found = s.graph.neighbours(lib, &focus.key, 100).await.unwrap();
    let titles: Vec<&str> = found.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, vec!["Rare too"], "only the meaningful tag makes an edge");
}

#[tokio::test]
async fn relates_items_by_their_leading_author() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let focus = s.items.create(lib, by("Focus", "Vaswani")).await.unwrap();
    s.items.create(lib, by("Same author", "Vaswani")).await.unwrap();
    s.items.create(lib, by("Someone else", "Shazeer")).await.unwrap();

    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Same author");
    assert_eq!(found[0].relation, crate::Relation::Author);
}

#[tokio::test]
async fn an_item_with_no_author_is_not_related_to_every_other_one() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    // Empty is not a name. Matching on it would join every anonymous item to
    // every other, which is the loudest possible wrong answer.
    let focus = s.items.create(lib, ItemDraft::new("book").with_field("title", "A")).await.unwrap();
    s.items.create(lib, ItemDraft::new("book").with_field("title", "B")).await.unwrap();

    assert!(s.graph.neighbours(lib, &focus.key, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn relates_items_filed_together() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let collection = s
        .collections
        .create(
            lib,
            yk_core::model::CollectionDraft { name: "Reading".into(), ..Default::default() },
        )
        .await
        .unwrap();

    let focus = s.items.create(lib, ItemDraft::new("book").with_field("title", "A")).await.unwrap();
    let other = s.items.create(lib, ItemDraft::new("book").with_field("title", "B")).await.unwrap();
    s.items.add_to_collection(lib, &collection.key, &[focus.key.clone(), other.key]).await.unwrap();

    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].relation, crate::Relation::Collection);
}

#[tokio::test]
async fn never_relates_an_item_to_itself() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let focus = s.items.create(lib, tagged("Focus", &["transformer"])).await.unwrap();
    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    assert!(!found.iter().any(|n| n.key == focus.key));
}

#[tokio::test]
async fn leaves_out_what_the_library_no_longer_shows() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let focus = s.items.create(lib, tagged("Focus", &["transformer"])).await.unwrap();
    let gone = s.items.create(lib, tagged("Trashed", &["transformer"])).await.unwrap();
    s.items.set_trashed(lib, std::slice::from_ref(&gone.key), true).await.unwrap();

    // A graph that shows what the list does not is a graph nobody can trust.
    assert!(s.graph.neighbours(lib, &focus.key, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn asking_about_an_item_that_is_not_here_says_so() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let missing = yk_core::Key::generate();

    let err = s.graph.neighbours(lib, &missing, 10).await.unwrap_err();
    assert!(err.to_string().contains(missing.as_str()), "{err}");
}

/// What SQLite says it will do, rather than what it returns.
///
/// Parameters are bound so the planner sees the same statement the repository
/// runs; their values are irrelevant to the plan but their presence is not.
fn plan(sql: &str, params: usize) -> String {
    let store = Store::in_memory().unwrap();
    let conn = store.db().conn().unwrap();
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let bound: Vec<i64> = (0..params).map(|_| 1).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bound), |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
    rows.join(" | ")
}

#[test]
fn the_tag_query_is_driven_by_the_tags_not_by_the_library() {
    // `CROSS JOIN` in these queries is not decoration and is invisible to every
    // other test here, so it will look like something to tidy away. Left to
    // itself the planner drove this from `items` on `parent_id IS NULL` — a
    // predicate matching the whole library — and probed tags for each of a
    // hundred thousand rows: 61 ms against 8.
    let plan = plan(crate::graph::TAG_SQL, 4);
    assert!(plan.starts_with("SEARCH it USING"), "the tag index must be the outer loop: {plan}");
    assert!(!plan.contains("SCAN i"), "{plan}");
}

#[test]
fn the_collection_query_is_driven_by_the_memberships() {
    // The same, and it mattered more: 28 ms to walk the library and report that
    // an item in no collection has no shelf-mates.
    let plan = plan(crate::graph::COLLECTION_SQL, 3);
    assert!(plan.starts_with("SEARCH ci USING"), "{plan}");
    assert!(!plan.contains("SCAN i"), "{plan}");
}

#[test]
fn the_author_query_seeks_by_name_before_it_sorts_by_year() {
    // Sorting first tempts the planner to walk the year index looking for a
    // name, which costs 150 ms for an author with no other works — the common
    // case, since most authors appear once.
    let plan = plan(crate::graph::AUTHOR_SQL, 5);
    assert!(plan.contains("idx_items_creator"), "must seek by name: {plan}");
}
