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

#[tokio::test]
async fn finds_papers_whose_references_have_never_been_fetched() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let waiting = s.items.create(lib, paper("Waiting", Some("10.1/waiting"))).await.unwrap();
    let done = s.items.create(lib, paper("Done", Some("10.1/done"))).await.unwrap();
    s.items.create(lib, paper("No identifier", None)).await.unwrap();
    s.relations.set_citations(lib, &done.key, vec![cite("", "Something")]).await.unwrap();

    let pending = s.relations.unfetched(lib, 10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, waiting.key);
    // The DOI as the item stores it: the fingerprint cannot be turned back
    // into an address to fetch.
    assert_eq!(pending[0].1, "10.1/waiting");
}

#[tokio::test]
async fn a_paper_whose_publisher_deposited_nothing_is_not_asked_about_twice() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let item = s.items.create(lib, paper("Empty bibliography", Some("10.1/none"))).await.unwrap();

    // Crossref answered, and the answer was "no references". That leaves no
    // rows at all, so only a record of the asking distinguishes it from never
    // having asked — and without it a bulk run would re-ask about every such
    // paper on every run, forever, against somebody else's free service.
    s.relations.set_citations(lib, &item.key, vec![]).await.unwrap();

    assert!(s.relations.unfetched(lib, 10).await.unwrap().is_empty());
}

/// What the maintained count would be if it were computed from scratch.
async fn counted_afresh(s: &Store) -> Vec<(String, i64)> {
    let conn = s.db().conn().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT target_key, count(DISTINCT source_id) FROM item_relations
             WHERE kind = 'cites' AND target_key != ''
             GROUP BY target_key ORDER BY target_key",
        )
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

/// What the maintained table says.
async fn counted_as_kept(s: &Store) -> Vec<(String, i64)> {
    let conn = s.db().conn().unwrap();
    let mut stmt = conn
        .prepare("SELECT target_key, citations FROM cited_works ORDER BY target_key")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[tokio::test]
async fn the_kept_count_never_disagrees_with_what_it_counts() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();
    let b = s.items.create(lib, paper("B", Some("10.1/b"))).await.unwrap();
    let shared = print_of("10.1/shared");
    let only_a = print_of("10.1/onlya");

    // A count kept beside the thing it counts is only defensible if it cannot
    // drift from it, so the test is that the two always agree — after adding,
    // after replacing, after emptying.
    s.relations
        .set_citations(lib, &a.key, vec![cite(&shared, "Shared"), cite(&only_a, "Only A")])
        .await
        .unwrap();
    assert_eq!(counted_as_kept(&s).await, counted_afresh(&s).await);

    s.relations.set_citations(lib, &b.key, vec![cite(&shared, "Shared")]).await.unwrap();
    assert_eq!(counted_as_kept(&s).await, counted_afresh(&s).await);

    // Replacing a bibliography takes its old votes back.
    s.relations.set_citations(lib, &a.key, vec![cite(&shared, "Shared")]).await.unwrap();
    assert_eq!(counted_as_kept(&s).await, counted_afresh(&s).await);

    s.relations.set_citations(lib, &a.key, vec![]).await.unwrap();
    s.relations.set_citations(lib, &b.key, vec![]).await.unwrap();
    assert_eq!(counted_as_kept(&s).await, counted_afresh(&s).await);
}

#[tokio::test]
async fn a_work_nobody_cites_any_more_is_not_a_row() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();

    s.relations
        .set_citations(lib, &a.key, vec![cite(&print_of("10.1/x"), "X")])
        .await
        .unwrap();
    s.relations.set_citations(lib, &a.key, vec![]).await.unwrap();

    // Zero citations is not a fact worth storing; it is the absence of one.
    assert!(counted_as_kept(&s).await.is_empty());
}

#[tokio::test]
async fn keeps_a_title_that_arrives_in_a_later_bibliography() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    let a = s.items.create(lib, paper("A", Some("10.1/a"))).await.unwrap();
    let b = s.items.create(lib, paper("B", Some("10.1/b"))).await.unwrap();
    let key = print_of("10.1/target");

    // The first publisher deposited only an identifier; the second wrote out
    // the title. Keeping the better label is the point of merging them.
    s.relations
        .set_citations(
            lib,
            &a.key,
            vec![CitationDraft {
                fingerprint: key.clone(),
                doi: "10.1/target".into(),
                label: String::new(),
                year: None,
            }],
        )
        .await
        .unwrap();
    s.relations.set_citations(lib, &b.key, vec![cite(&key, "The real title")]).await.unwrap();

    assert_eq!(s.relations.missing(lib, 10).await.unwrap()[0].label, "The real title");
}

#[test]
fn the_missing_query_asks_the_index_that_can_answer() {
    // Invisible and load-bearing. The planner's own choice — an index on
    // `(library_id, deleted)` followed by a scan for the fingerprint — returns
    // exactly the same rows, and took 8.5 seconds instead of 0.2 milliseconds
    // on a library with 1.8 million references. Only a plan assertion can catch
    // somebody tidying the hint away.
    let store = Store::in_memory().unwrap();
    let conn = store.db().conn().unwrap();
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {}", crate::relations::MISSING_SQL))
        .unwrap();
    let plan = stmt
        .query_map(rusqlite::params![1i64, 10i64], |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>()
        .join(" | ");

    assert!(plan.contains("idx_items_fingerprint"), "{plan}");
    assert!(plan.contains("idx_cited_works_rank"), "the ranking must stream: {plan}");
}

#[test]
fn a_bibliography_resolves_through_the_fingerprint_index() {
    // Same fault as `MISSING_SQL`, found separately and much later: left to
    // itself the planner joined through `idx_items_year` — a predicate
    // matching the whole library — and scanned it once per reference. 1585 ms
    // against 0.14 ms for thirty rows, with identical results. Every test that
    // checks only the answer passed throughout.
    let store = Store::in_memory().unwrap();
    let conn = store.db().conn().unwrap();
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {}", crate::relations::CITES_SQL))
        .unwrap();
    let plan = stmt
        .query_map(rusqlite::params![1i64, 1i64, "cites"], |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>()
        .join(" | ");

    assert!(plan.contains("idx_items_fingerprint"), "{plan}");
    assert!(!plan.contains("idx_items_year"), "{plan}");
}
