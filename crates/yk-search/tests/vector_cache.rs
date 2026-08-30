//! Vectors of deleted items must not stay in memory.
//!
//! The cache is a mirror of `item_vectors` and was only ever written to. Rows
//! leave that table by cascade when an item is deleted, so the mirror drifted:
//! a scratch library holding 26 documents reported 99,719 embedded, and would
//! have kept growing for as long as the server ran. Restarting "fixed" it,
//! which is the tell — startup is the only thing that rebuilt the cache.

use yk_core::model::ItemDraft;
use yk_core::ports::SearchIndex;
use yk_search::{LocalEmbedder, SearchEngine};
use yk_store::Store;

use std::sync::Arc;

async fn embed_everything(engine: &SearchEngine) {
    while engine.embed_pending(100).await.unwrap() > 0 {}
}

#[tokio::test]
async fn deleting_items_releases_their_vectors() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();

    let mut keys = Vec::new();
    for i in 0..12 {
        let draft = ItemDraft::new("journalArticle")
            .with_field("title", format!("A paper about attention number {i}"));
        keys.push(store.items.create(lib, draft).await.unwrap().key);
    }
    embed_everything(&engine).await;
    assert_eq!(engine.stats().await.unwrap().embedded, 12, "nothing was embedded to begin with");

    store.items.delete(lib, &keys[..8]).await.unwrap();

    // The worker's next pass is what notices. It must not need a restart, and
    // it must not need anything to be queued: deleting is not an edit that
    // asks for an embedding, so there may be no pending work at all.
    embed_everything(&engine).await;

    assert_eq!(
        engine.stats().await.unwrap().embedded,
        4,
        "vectors of deleted items were still held in memory"
    );
}
