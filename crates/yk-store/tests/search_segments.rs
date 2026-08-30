//! The search index needs compacting, and what was doing it did not work.
//!
//! Every write appends to an FTS5 index, and the pages accumulate. Measured on
//! a real 100,000-item library after a few weeks of ordinary use: the index's
//! storage table held 30,770 rows, and merging it down to 6,342 took a common
//! keyword search from 30.6ms to 23.9ms. The only maintenance this database
//! had was `PRAGMA optimize`, which refreshes the query planner's statistics —
//! a different thing with a confusingly similar name.
//!
//! A library that is used gets slower at being searched, for as long as it is
//! used, with no way back short of a full reindex.
//!
//! FTS5 merges a little on its own (`automerge`, four segments by default),
//! which keeps a small index tidy and is why this has to be provoked to be
//! tested at all — and why it still went wrong on a library of a hundred
//! thousand.
//!
//! One honest limitation. These tests cannot tell the *incremental* merge
//! apart from `optimize`: at four hundred items both gather the index into a
//! single segment, which is why the version of this file that tested the
//! incremental merge passed for several rounds while that merge was achieving
//! nothing on the real library. The difference only appears in the multi-level
//! shape a large index reaches over months, and the evidence for it is the
//! measurement recorded on `compact_search_indexes` — 19,362 pages to 6,350,
//! and a keyword search from 26.6ms to 22.2ms.
//!
//! So what is guarded here is narrower than the bug that prompted it: that
//! compaction compacts, and that the index still answers afterwards.

use yk_core::model::ItemDraft;
use yk_store::Store;

/// How much storage the full-text index is spread across.
///
/// Not a segment count -- that lives in a blob -- but the number of pages the
/// index occupies, which is what a query has to read through.
fn pages(store: &Store) -> i64 {
    let conn = store.db().conn().unwrap();
    conn.query_row("SELECT count(*) FROM items_fts_data", [], |r| r.get(0)).unwrap()
}

/// Stop FTS5 tidying up behind us, so the state a real library reaches over
/// months is reached here in a second.
fn stop_automatic_merging(store: &Store) {
    let conn = store.db().conn().unwrap();
    conn.execute("INSERT INTO items_fts(items_fts, rank) VALUES('automerge', 0)", []).unwrap();
}

/// Written one at a time, which is how a library is really written: a paper
/// added, a note taken, a tag changed. A bulk insert produces one segment and
/// would show nothing.
async fn write_one_at_a_time(store: &Store, lib: i64, n: usize) {
    for i in 0..n {
        let draft = ItemDraft::new("journalArticle")
            .with_field("title", format!("A paper about attention number {i}"))
            .with_field("abstractNote", format!("We present method {i} for reading papers."));
        store.items.create(lib, draft).await.unwrap();
    }
}

#[tokio::test]
async fn writing_spreads_the_index_out_and_merging_gathers_it_back() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    stop_automatic_merging(&store);
    write_one_at_a_time(&store, lib, 400).await;
    let spread = pages(&store);

    store.db().compact_search_indexes().await.unwrap();
    let gathered = pages(&store);

    assert!(gathered < spread, "compaction did nothing: {spread} pages before and after");

    // And it reached the bottom rather than merely moving towards it. Pages
    // cannot fall below the size of the text itself, so the assertion is not a
    // ratio but a fixed point: compacting a compacted index changes nothing.
    // An implementation that leaves work behind keeps shrinking here.
    store.db().compact_search_indexes().await.unwrap();
    assert_eq!(pages(&store), gathered, "compaction left work behind for a second pass");

    // And it is still the same index afterwards, which is what would matter if
    // a merge ever lost anything.
    let conn = store.db().conn().unwrap();
    let hits: i64 = conn
        .query_row("SELECT count(*) FROM items_fts WHERE items_fts MATCH 'attention'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(hits, 400, "every paper is still findable");
}

/// It must be safe to run on a timer, which means costing nothing when there
/// is nothing to do. Measured on the real library: a second `optimize` on a
/// compacted index takes 0.00s and reports a single change.
#[tokio::test]
async fn merging_an_already_merged_index_is_harmless() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    write_one_at_a_time(&store, lib, 20).await;

    for _ in 0..5 {
        store.db().compact_search_indexes().await.unwrap();
    }
    // And the index still answers afterwards, which is the thing that would
    // matter if a merge ever corrupted it.
    let conn = store.db().conn().unwrap();
    let hits: i64 = conn
        .query_row(
            "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'attention'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hits, 20, "every paper is still findable");
}

/// An empty library has no segments to merge and must not error, since the
/// worker runs on a timer from the moment the server starts.
#[tokio::test]
async fn merging_an_empty_library_is_not_an_error() {
    let store = Store::in_memory().unwrap();
    store.db().compact_search_indexes().await.unwrap();
}
