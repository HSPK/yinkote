//! Tests for the relationship graph.

use crate::Store;
use yk_core::model::{Creator, ItemDraft, ItemTag};

fn tagged(title: &str, tags: &[&str]) -> ItemDraft {
    let mut draft = ItemDraft::new("journalArticle").with_field("title", title);
    draft.tags = tags.iter().map(|t| ItemTag { tag: (*t).into(), r#type: 0 }).collect();
    draft
}

fn plain(title: &str) -> ItemDraft {
    ItemDraft::new("journalArticle").with_field("title", title)
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

// ---------------------------------------------------------------------------
// Bibliographic coupling
// ---------------------------------------------------------------------------

use crate::relations::CitationDraft;

/// A reference the publisher gave an identifier to.
fn cites(doi: &str) -> CitationDraft {
    CitationDraft {
        fingerprint: format!("doi:{doi}"),
        doi: doi.into(),
        label: format!("Work {doi}"),
        year: Some(2020),
    }
}

#[tokio::test]
async fn relates_papers_that_lean_on_the_same_references() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let focus = s.items.create(lib, plain("Focus")).await.unwrap();
    let close = s.items.create(lib, plain("Reads the same things")).await.unwrap();
    let brushes = s.items.create(lib, plain("Shares only one")).await.unwrap();
    let apart = s.items.create(lib, plain("Reads something else")).await.unwrap();

    let shared = vec![cites("10.1/a"), cites("10.1/b"), cites("10.1/c")];
    s.relations.set_citations(lib, &focus.key, shared.clone()).await.unwrap();
    s.relations.set_citations(lib, &close.key, shared).await.unwrap();
    s.relations
        .set_citations(lib, &brushes.key, vec![cites("10.1/a"), cites("10.9/x")])
        .await
        .unwrap();
    s.relations.set_citations(lib, &apart.key, vec![cites("10.9/y")]).await.unwrap();

    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    let coupled: Vec<_> =
        found.iter().filter(|n| n.relation == crate::graph::Relation::Coupling).collect();

    // Two papers citing the same three works are working on the same problem,
    // whether or not anybody has tagged them that way.
    assert_eq!(coupled.len(), 1, "{coupled:?}");
    assert_eq!(coupled[0].title, "Reads the same things");
    assert_eq!(coupled[0].weight, 3.0);

    // One shared reference is a coincidence — two papers in a field share a
    // review without being about the same thing.
    let titles: Vec<&str> = coupled.iter().map(|n| n.title.as_str()).collect();
    assert!(!titles.contains(&"Shares only one"), "{titles:?}");
    assert!(!titles.contains(&"Reads something else"), "{titles:?}");
}

#[tokio::test]
async fn a_reference_everybody_cites_draws_no_edges() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    // Every paper in a field cites its founding text. An edge drawn from that
    // is an edge between everything and everything.
    let founding = vec![cites("10.1/founding"), cites("10.1/second")];
    let focus = s.items.create(lib, plain("Focus")).await.unwrap();
    s.relations.set_citations(lib, &focus.key, founding.clone()).await.unwrap();

    for i in 0..60 {
        let other = s.items.create(lib, plain(&format!("Paper {i}"))).await.unwrap();
        s.relations.set_citations(lib, &other.key, founding.clone()).await.unwrap();
    }

    let found = s.graph.neighbours(lib, &focus.key, 100).await.unwrap();
    let coupled = found.iter().filter(|n| n.relation == crate::graph::Relation::Coupling).count();
    assert_eq!(coupled, 0, "a reference on sixty papers relates none of them");
}

#[tokio::test]
async fn a_reference_with_no_identifier_couples_nothing() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    // Two bibliographies rarely spell the same paper the same way, so an
    // entry with no DOI would couple on a typo — a wrong edge stated with
    // confidence, which is worse than a missing one.
    let vague = |label: &str| CitationDraft {
        fingerprint: String::new(),
        doi: String::new(),
        label: label.into(),
        year: Some(1999),
    };
    let focus = s.items.create(lib, plain("Focus")).await.unwrap();
    let other = s.items.create(lib, plain("Other")).await.unwrap();
    s.relations
        .set_citations(lib, &focus.key, vec![vague("Smith 1999"), vague("Jones 1998")])
        .await
        .unwrap();
    s.relations
        .set_citations(lib, &other.key, vec![vague("Smith 1999"), vague("Jones 1998")])
        .await
        .unwrap();

    let found = s.graph.neighbours(lib, &focus.key, 10).await.unwrap();
    assert!(found.iter().all(|n| n.relation != crate::graph::Relation::Coupling), "{found:?}");
}

#[test]
fn the_coupling_query_is_driven_by_the_references_not_by_the_library() {
    // Same class as the tag query: left alone the planner will happily scan
    // every item and probe the reference table for each one.
    let plan = plan(crate::graph::COUPLING_SQL, 5);
    assert!(plan.contains("SEARCH theirs"), "the reference index must lead: {plan}");
    assert!(!plan.contains("SCAN i"), "{plan}");
}
