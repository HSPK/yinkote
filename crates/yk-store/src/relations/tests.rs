//! Tests for stored citations.

use super::CitationDraft;
use crate::Store;
use yk_core::model::ItemDraft;

fn paper(title: &str, doi: Option<&str>) -> ItemDraft {
    let mut draft = ItemDraft::new("journalArticle").with_field("title", title);
    if let Some(doi) = doi {
        draft = draft.with_field("DOI", doi);
    }
    draft
}

fn cite(fingerprint: &str, label: &str) -> CitationDraft {
    CitationDraft {
        fingerprint: fingerprint.into(),
        doi: fingerprint.strip_prefix("doi:").unwrap_or_default().into(),
        label: label.into(),
        year: Some(2015),
    }
}

/// The fingerprint an item with this DOI will have.
fn print_of(doi: &str) -> String {
    format!("doi:{}", yk_core::text::normalize(doi))
}

#[tokio::test]
async fn keeps_a_bibliography_in_the_order_it_was_printed() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let item = s.items.create(lib, paper("Source", Some("10.1/src"))).await.unwrap();

    s.relations
        .set_citations(
            lib,
            &item.key,
            vec![cite("", "First"), cite("", "Second"), cite("", "Third")],
        )
        .await
        .unwrap();

    let cited = s.relations.cites(lib, &item.key).await.unwrap();
    assert_eq!(cited.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["First", "Second", "Third"]);
}

#[tokio::test]
async fn a_reference_to_something_not_owned_is_still_a_reference() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let item = s.items.create(lib, paper("Source", Some("10.1/src"))).await.unwrap();

    s.relations
        .set_citations(lib, &item.key, vec![cite(&print_of("10.1/absent"), "A paper nobody has")])
        .await
        .unwrap();

    let cited = s.relations.cites(lib, &item.key).await.unwrap();
    // Most cited works are not in the library. Showing what is missing is the
    // point: a paper cited often and owned never is required reading.
    assert_eq!(cited.len(), 1);
    assert!(cited[0].key.is_none());
    assert_eq!(cited[0].label, "A paper nobody has");
}

#[tokio::test]
async fn a_reference_resolves_by_itself_when_the_paper_arrives_later() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let source = s.items.create(lib, paper("Source", Some("10.1/src"))).await.unwrap();

    s.relations
        .set_citations(lib, &source.key, vec![cite(&print_of("10.1/target"), "Cited work")])
        .await
        .unwrap();
    assert!(s.relations.cites(lib, &source.key).await.unwrap()[0].key.is_none());

    // Acquired afterwards, which is the ordinary way a library grows.
    let target = s.items.create(lib, paper("Cited work", Some("10.1/target"))).await.unwrap();

    // No backfill ran. Resolution happens when the graph is read, so there is
    // no window in which the library holds both papers and draws them as
    // strangers.
    let cited = s.relations.cites(lib, &source.key).await.unwrap();
    assert_eq!(cited[0].key.as_ref(), Some(&target.key));
}

#[tokio::test]
async fn asks_the_same_question_backwards() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let source = s.items.create(lib, paper("Citing paper", Some("10.1/src"))).await.unwrap();
    let target = s.items.create(lib, paper("Cited work", Some("10.1/target"))).await.unwrap();

    s.relations
        .set_citations(lib, &source.key, vec![cite(&print_of("10.1/target"), "Cited work")])
        .await
        .unwrap();

    let citing = s.relations.cited_by(lib, &target.key).await.unwrap();
    assert_eq!(citing.len(), 1);
    assert_eq!(citing[0].key.as_ref(), Some(&source.key));
    assert_eq!(citing[0].label, "Citing paper");
}

#[tokio::test]
async fn an_item_with_no_identifier_is_not_claimed_to_be_uncited() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let anonymous = s.items.create(lib, paper("No DOI here", None)).await.unwrap();

    // It cannot be recognised in anybody's bibliography. Returning nothing is
    // honest; matching on title would be wrong sometimes, which is worse.
    assert!(s.relations.cited_by(lib, &anonymous.key).await.unwrap().is_empty());
}

#[tokio::test]
async fn replacing_a_bibliography_does_not_merge_two_versions_of_it() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let item = s.items.create(lib, paper("Source", Some("10.1/src"))).await.unwrap();

    s.relations
        .set_citations(lib, &item.key, vec![cite("", "Old first"), cite("", "Old second")])
        .await
        .unwrap();
    s.relations.set_citations(lib, &item.key, vec![cite("", "Only one now")]).await.unwrap();

    let cited = s.relations.cites(lib, &item.key).await.unwrap();
    assert_eq!(cited.len(), 1, "a bibliography is one thing, not an accumulation");
}

#[tokio::test]
async fn citations_go_when_the_item_does() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let item = s.items.create(lib, paper("Source", Some("10.1/src"))).await.unwrap();
    s.relations.set_citations(lib, &item.key, vec![cite("", "Something")]).await.unwrap();

    s.items.set_trashed(lib, std::slice::from_ref(&item.key), true).await.unwrap();
    s.items.empty_trash(lib).await.unwrap();

    // The row is gone with its item; a dangling bibliography would be a leak
    // that only shows up as a graph edge from nowhere.
    let conn = s.db().conn().unwrap();
    let left: i64 =
        conn.query_row("SELECT count(*) FROM item_relations", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn a_trashed_citing_paper_does_not_show_up_as_a_citation() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let source = s.items.create(lib, paper("Citing paper", Some("10.1/src"))).await.unwrap();
    let target = s.items.create(lib, paper("Cited work", Some("10.1/target"))).await.unwrap();
    s.relations
        .set_citations(lib, &source.key, vec![cite(&print_of("10.1/target"), "Cited work")])
        .await
        .unwrap();

    s.items.set_trashed(lib, std::slice::from_ref(&source.key), true).await.unwrap();

    // A graph that shows what the list does not is a graph nobody can trust.
    assert!(s.relations.cited_by(lib, &target.key).await.unwrap().is_empty());
}

#[tokio::test]
async fn ranks_the_works_the_library_keeps_citing_and_does_not_have() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();
    let b = s.items.create(lib, paper("B", Some("10.1/b"))).await.unwrap();
    let c = s.items.create(lib, paper("C", Some("10.1/c"))).await.unwrap();

    let popular = print_of("10.1/popular");
    let obscure = print_of("10.1/obscure");
    for item in [&a, &b, &c] {
        s.relations
            .set_citations(lib, &item.key, vec![cite(&popular, "Everyone cites this")])
            .await
            .unwrap();
    }
    s.relations
        .set_citations(lib, &a.key, vec![cite(&popular, "Everyone cites this"), cite(&obscure, "Only A cites this")])
        .await
        .unwrap();

    let missing = s.relations.missing(lib, 10).await.unwrap();
    assert_eq!(missing[0].label, "Everyone cites this");
    assert_eq!(missing[0].cited_by, 3);
    assert_eq!(missing[1].cited_by, 1);
}

#[tokio::test]
async fn a_work_the_library_owns_is_not_missing() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();
    s.items.create(lib, paper("Owned", Some("10.1/owned"))).await.unwrap();

    s.relations
        .set_citations(lib, &a.key, vec![cite(&print_of("10.1/owned"), "Owned")])
        .await
        .unwrap();

    assert!(s.relations.missing(lib, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn counts_citing_papers_rather_than_reference_entries() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();

    let twice = print_of("10.1/twice");
    s.relations
        .set_citations(lib, &a.key, vec![cite(&twice, "Listed twice"), cite(&twice, "Listed twice")])
        .await
        .unwrap();

    // A bibliography that lists the same work twice says nothing about how
    // central it is.
    assert_eq!(s.relations.missing(lib, 10).await.unwrap()[0].cited_by, 1);
}

#[tokio::test]
async fn leaves_out_references_that_cannot_be_grouped() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();

    // Prose references have no identifier: the same paper appears in ten
    // bibliographies in ten house styles, so counting them would rank
    // formatting conventions rather than papers.
    s.relations
        .set_citations(lib, &a.key, vec![cite("", "Somebody, in a journal, 2001.")])
        .await
        .unwrap();

    assert!(s.relations.missing(lib, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_trashed_paper_stops_voting() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();
    s.relations
        .set_citations(lib, &a.key, vec![cite(&print_of("10.1/x"), "Cited by a trashed paper")])
        .await
        .unwrap();

    s.items.set_trashed(lib, std::slice::from_ref(&a.key), true).await.unwrap();
    assert!(s.relations.missing(lib, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn keeps_the_identifier_the_publisher_wrote() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();

    s.relations
        .set_citations(
            lib,
            &a.key,
            vec![CitationDraft {
                fingerprint: print_of("10.1016/j.cell.2020.01.001"),
                doi: "10.1016/j.cell.2020.01.001".into(),
                label: "A cell paper".into(),
                year: Some(2020),
            }],
        )
        .await
        .unwrap();

    // The fingerprint flattens punctuation so two spellings match, which makes
    // it one-way: this DOI cannot be reconstructed from it. Fetching the paper
    // needs the original, so the original is kept.
    let missing = s.relations.missing(lib, 10).await.unwrap();
    assert_eq!(missing[0].doi, "10.1016/j.cell.2020.01.001");
}
