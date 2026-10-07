use std::sync::Arc;

use yk_core::model::{Creator, ItemDraft, ItemTag};
use yk_core::ports::SearchIndex;
use yk_core::query::{ItemFilter, SearchMode, SearchRequest};
use yk_search::{LocalEmbedder, SearchEngine};
use yk_store::Store;

fn request(lib: i64, text: &str, mode: SearchMode) -> SearchRequest {
    SearchRequest {
        text: text.into(),
        mode,
        filter: ItemFilter {
            library_id: lib,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn author_year_and_phrase_are_constraints_in_every_mode() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    let mut wanted = None;
    for (title, author, year) in [
        ("Diffusion models for science", "Zhang", "2024"),
        ("Diffusion models for science", "Zhang", "2020"),
        ("Diffusion models for science", "Smith", "2024"),
        ("Models for diffusion science", "Zhang", "2024"),
        ("Diffusion modelling for science", "Zhang", "2024"),
    ] {
        let item = store
            .items
            .create(
                lib,
                ItemDraft::new("journalArticle")
                    .with_field("title", title)
                    .with_field("date", year)
                    .with_creator(Creator::author("Wei", author)),
            )
            .await
            .unwrap();
        wanted.get_or_insert(item.key);
    }
    engine.embed_pending(100).await.unwrap();
    for mode in [
        SearchMode::Keyword,
        SearchMode::Fuzzy,
        SearchMode::Semantic,
        SearchMode::Hybrid,
    ] {
        let page = engine
            .search(&request(
                lib,
                "\"diffusion models\" author:zhang year:2024",
                mode,
            ))
            .await
            .unwrap();
        assert_eq!(page.hits.len(), 1, "{mode:?}");
        assert_eq!(Some(&page.hits[0].key), wanted.as_ref());
    }
    let mut query = request(lib, "author:zhang year:2024", SearchMode::Hybrid);
    query.limit = 1;
    query.offset = 100;
    let page = engine.search(&query).await.unwrap();
    assert_eq!(page.total, 3);
    assert!(page.hits.is_empty());
    assert!(!page.capped);
}

#[tokio::test]
async fn filters_apply_before_the_ranked_candidate_limit() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    let drafts = (0..350)
        .map(|_| ItemDraft::new("journalArticle").with_field("title", "Diffusion"))
        .collect();
    store.items.create_many(lib, drafts).await.unwrap();
    let mut draft = ItemDraft::new("journalArticle").with_field(
        "title",
        "Diffusion models for a particularly long scientific study",
    );
    draft.tags = vec![ItemTag::manual("wanted")];
    let wanted = store.items.create(lib, draft).await.unwrap();
    for mode in [SearchMode::Keyword, SearchMode::Fuzzy] {
        let page = engine
            .search(&request(lib, "diffusion tag:wanted", mode))
            .await
            .unwrap();
        assert_eq!(page.hits.len(), 1, "{mode:?}");
        assert_eq!(page.hits[0].key, wanted.key);
        assert!(!page.capped);
    }
}

#[tokio::test]
async fn semantic_filter_does_not_stop_after_twenty_thousand_items() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    let mut draft = ItemDraft::new("journalArticle").with_field("title", "Rare diffusion research");
    draft.tags = vec![ItemTag::manual("shelf")];
    let wanted = store.items.create(lib, draft).await.unwrap();
    engine.embed_pending(1).await.unwrap();
    // Only the structural set needs scale; avoid spending this regression on
    // generating twenty thousand unrelated full-text indexes.
    store
        .db()
        .call(move |c| {
            c.execute_batch(
                "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<20001)
             INSERT INTO items(library_id,key,item_type,date_added,date_modified)
             SELECT 1, printf('%08d',x), 'journalArticle', x, 9999999999999 FROM n;
             INSERT INTO item_tags(item_id,tag_id,type)
             SELECT i.id,t.id,0 FROM items i JOIN tags t ON t.name='shelf'
             WHERE i.date_modified=9999999999999;",
            )
            .map_err(yk_store::sql_err)
        })
        .await
        .unwrap();
    let page = engine
        .search(&request(lib, "diffusion tag:shelf", SearchMode::Semantic))
        .await
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].key, wanted.key);
}

#[tokio::test]
async fn chinese_phrases_preserve_their_order() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    let wanted = store
        .items
        .create(
            lib,
            ItemDraft::new("journalArticle").with_field("title", "扩散模型研究"),
        )
        .await
        .unwrap();
    store
        .items
        .create(
            lib,
            ItemDraft::new("journalArticle").with_field("title", "模型扩散研究"),
        )
        .await
        .unwrap();
    let page = engine
        .search(&request(lib, "\"扩散模型\"", SearchMode::Hybrid))
        .await
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].key, wanted.key);
}

#[tokio::test]
async fn a_library_write_invalidates_cached_semantic_eligibility() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    let item = store
        .items
        .create(
            lib,
            ItemDraft::new("journalArticle").with_field("title", "Diffusion models"),
        )
        .await
        .unwrap();
    engine.embed_pending(10).await.unwrap();
    let query = request(lib, "diffusion", SearchMode::Semantic);
    for _ in 0..2 {
        assert_eq!(engine.search(&query).await.unwrap().hits.len(), 1);
    }
    store
        .items
        .set_trashed(lib, std::slice::from_ref(&item.key), true)
        .await
        .unwrap();
    assert!(engine.search(&query).await.unwrap().hits.is_empty());
    store
        .items
        .set_trashed(lib, std::slice::from_ref(&item.key), false)
        .await
        .unwrap();
    assert_eq!(engine.search(&query).await.unwrap().hits.len(), 1);
    store.items.delete(lib, &[item.key]).await.unwrap();
    assert!(engine.search(&query).await.unwrap().hits.is_empty());
}
