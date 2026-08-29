//! Guards fuzzy search against losing the paper the reader actually named.
//!
//! Stage 1 of [`yk_search::lexical::fuzzy`] asks for the query as an exact
//! phrase; stage 2 falls back to overlapping chunks. Stage 2 truncates its
//! candidates in rowid order, so on a library of any size the exact match is
//! not reliably among them — which makes stage 1 the only thing guaranteeing
//! that searching a title returns that title.
//!
//! The unit tests beside the engine cannot show this: they seed four
//! documents, so nothing ever truncates and they pass on an implementation
//! that drops stage 1 entirely. This one seeds past the truncation point.

use yk_core::model::ItemDraft;
use yk_search::lexical;
use yk_store::Store;

const LIMIT: usize = 4;

/// A library where the chunk fallback cannot help: many titles sharing the
/// query's leading chunk, and the wanted one added last so it has the highest
/// rowid and is the first thing a truncation drops.
async fn library_of_near_misses(decoy: &str, wanted: &str) -> Store {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    for n in 0..(LIMIT * 8) {
        let draft = ItemDraft::new("journalArticle")
            .with_field("title", format!("{decoy} {n}"));
        store.items.create(lib, draft).await.unwrap();
    }
    store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", wanted))
        .await
        .unwrap();
    store
}

#[tokio::test]
async fn a_paper_is_found_by_its_whole_title_among_many_near_misses() {
    let store =
        library_of_near_misses("Attention mechanisms in vision, part", "Attention Is All You Need")
            .await;
    let conn = store.db().conn().unwrap();
    let found = lexical::fuzzy(&conn, store.default_library, "attention is all you need", LIMIT)
        .unwrap();

    let titles: Vec<&str> = found.iter().map(|c| c.title.as_str()).collect();
    assert!(
        titles.iter().any(|t| t.contains("all you need")),
        "the paper named by the query was not among the candidates: {titles:?}"
    );
}

/// The word index matches whole words, so without a trailing prefix a reader
/// who stops typing mid-word -- or types "diffusion model" for a paper about
/// diffusion *models* -- loses it. The trigram index this replaced gave that
/// for free.
#[tokio::test]
async fn a_phrase_still_matches_when_its_last_word_is_unfinished() {
    let store = library_of_near_misses(
        "Denoising methods for images, part",
        "Denoising Diffusion Probabilistic Models",
    )
    .await;
    let conn = store.db().conn().unwrap();
    let found =
        lexical::fuzzy(&conn, store.default_library, "denoising diffusion probabilistic mod", LIMIT)
            .unwrap();

    let titles: Vec<&str> = found.iter().map(|c| c.title.as_str()).collect();
    assert!(
        titles.iter().any(|t| t.contains("probabilistic models")),
        "an unfinished last word lost the paper: {titles:?}"
    );
}

#[tokio::test]
async fn a_misspelt_single_word_still_reaches_its_paper() {
    // The other half of the same function: one word takes the trigram index,
    // where a typo is cheap precisely because its trigrams are rare.
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    let draft = ItemDraft::new("journalArticle").with_field("title", "Attention Is All You Need");
    store.items.create(lib, draft).await.unwrap();

    let conn = store.db().conn().unwrap();
    let found = lexical::fuzzy(&conn, lib, "attentio", LIMIT).unwrap();
    assert!(found.iter().any(|c| c.title.contains("all you need")), "a typo lost its paper");
}
