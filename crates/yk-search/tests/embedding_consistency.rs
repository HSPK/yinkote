use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Semaphore;
use yk_ai::EmbeddingProvider;
use yk_core::model::{ItemDraft, ItemPatch};
use yk_core::ports::SearchIndex;
use yk_core::{Key, Result};
use yk_search::{LocalEmbedder, SearchEngine};
use yk_store::{sql_err, Store};

struct PausedEmbedder {
    entered: Semaphore,
    release: Semaphore,
    calls: AtomicUsize,
}

impl PausedEmbedder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            calls: AtomicUsize::new(0),
        })
    }

    async fn wait(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

#[async_trait]
impl EmbeddingProvider for PausedEmbedder {
    fn id(&self) -> &str {
        "paused-local"
    }
    fn dimensions(&self) -> usize {
        256
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
        LocalEmbedder::new().embed(texts).await
    }
}

async fn item(store: &Store, title: &str) -> Key {
    store
        .items
        .create(
            store.default_library,
            ItemDraft::new("journalArticle").with_field("title", title),
        )
        .await
        .unwrap()
        .key
}

async fn queued(store: &Store) -> i64 {
    store
        .db()
        .call(|c| {
            c.query_row("SELECT count(*) FROM embed_queue", [], |r| r.get(0))
                .map_err(sql_err)
        })
        .await
        .unwrap()
}

async fn stored(store: &Store) -> i64 {
    store
        .db()
        .call(|c| {
            c.query_row("SELECT count(*) FROM item_vectors", [], |r| r.get(0))
                .map_err(sql_err)
        })
        .await
        .unwrap()
}

fn start(engine: Arc<SearchEngine>) -> tokio::task::JoinHandle<Result<u32>> {
    tokio::spawn(async move { engine.embed_pending(100).await })
}

#[tokio::test]
async fn a_late_embedding_does_not_dequeue_a_newer_edit() {
    let store = Store::in_memory().unwrap();
    let key = item(&store, "Old title").await;
    let provider = PausedEmbedder::new();
    let engine = Arc::new(SearchEngine::new(store.clone(), provider.clone()).unwrap());
    let pass = start(engine.clone());
    provider.wait().await;
    store
        .items
        .update(
            store.default_library,
            &key,
            ItemPatch {
                fields: Some(
                    serde_json::from_value(serde_json::json!({ "title": "New title" })).unwrap(),
                ),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    provider.release.add_permits(1);
    assert_eq!(pass.await.unwrap().unwrap(), 0);
    assert_eq!(queued(&store).await, 1);
    assert_eq!(stored(&store).await, 0);
    assert_eq!(engine.stats().await.unwrap().embedded, 0);
    assert_eq!(engine.embed_pending(100).await.unwrap(), 1);
    assert_eq!(queued(&store).await, 0);
    let expected = yk_store::index::content_hash(&yk_store::index::embed_text(
        &store.items.get(store.default_library, &key).await.unwrap(),
    ));
    let actual: String = store
        .db()
        .call(|c| {
            c.query_row("SELECT content_hash FROM item_vectors", [], |r| r.get(0))
                .map_err(sql_err)
        })
        .await
        .unwrap();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn a_deleted_item_does_not_fail_the_rest_of_an_embedding_batch() {
    let store = Store::in_memory().unwrap();
    let doomed = item(&store, "Deleted").await;
    item(&store, "Kept").await;
    let provider = PausedEmbedder::new();
    let engine = Arc::new(SearchEngine::new(store.clone(), provider.clone()).unwrap());
    let pass = start(engine.clone());
    provider.wait().await;
    store
        .items
        .delete(store.default_library, &[doomed])
        .await
        .unwrap();
    provider.release.add_permits(1);
    assert_eq!(pass.await.unwrap().unwrap(), 1);
    assert_eq!(queued(&store).await, 0);
    assert_eq!(stored(&store).await, 1);
    assert_eq!(engine.stats().await.unwrap().embedded, 1);
}

#[tokio::test]
async fn trashing_during_embedding_cannot_publish_a_vector() {
    let store = Store::in_memory().unwrap();
    let key = item(&store, "Going to trash").await;
    let provider = PausedEmbedder::new();
    let engine = Arc::new(SearchEngine::new(store.clone(), provider.clone()).unwrap());
    let pass = start(engine.clone());
    provider.wait().await;
    store
        .items
        .set_trashed(store.default_library, &[key], true)
        .await
        .unwrap();
    provider.release.add_permits(1);
    assert_eq!(pass.await.unwrap().unwrap(), 0);
    assert_eq!(stored(&store).await, 0);
    assert_eq!(engine.stats().await.unwrap().embedded, 0);
}

#[tokio::test]
async fn a_failed_transaction_leaves_the_cache_and_queue_unchanged() {
    let store = Store::in_memory().unwrap();
    item(&store, "Retry me").await;
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    store
        .db()
        .call(|c| {
            c.execute_batch(
                "CREATE TRIGGER fail_vectors BEFORE INSERT ON item_vectors BEGIN
         SELECT RAISE(ABORT, 'simulated disk write failure'); END;",
            )
            .map_err(sql_err)
        })
        .await
        .unwrap();
    assert!(engine.embed_pending(100).await.is_err());
    assert_eq!(queued(&store).await, 1);
    assert_eq!(stored(&store).await, 0);
    assert_eq!(engine.stats().await.unwrap().embedded, 0);
    store
        .db()
        .call(|c| {
            c.execute_batch("DROP TRIGGER fail_vectors")
                .map_err(sql_err)
        })
        .await
        .unwrap();
    assert_eq!(engine.embed_pending(100).await.unwrap(), 1);
}

#[tokio::test]
async fn concurrent_passes_do_not_send_the_same_documents_twice() {
    let store = Store::in_memory().unwrap();
    item(&store, "One request").await;
    let provider = PausedEmbedder::new();
    let engine = Arc::new(SearchEngine::new(store.clone(), provider.clone()).unwrap());
    let first = start(engine.clone());
    provider.wait().await;
    let second = start(engine);
    provider.release.add_permits(1);
    assert_eq!(first.await.unwrap().unwrap(), 1);
    assert_eq!(second.await.unwrap().unwrap(), 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rebuilding_during_embedding_finishes_with_a_consistent_empty_cache() {
    let store = Store::in_memory().unwrap();
    item(&store, "Rebuild me").await;
    let provider = PausedEmbedder::new();
    let engine = Arc::new(SearchEngine::new(store.clone(), provider.clone()).unwrap());
    let first = start(engine.clone());
    provider.wait().await;
    let rebuilding = engine.clone();
    let reindex = tokio::spawn(async move { rebuilding.reindex(1).await });
    provider.release.add_permits(1);
    first.await.unwrap().unwrap();
    assert_eq!(reindex.await.unwrap().unwrap(), 1);
    assert_eq!(stored(&store).await, 0);
    assert_eq!(queued(&store).await, 1);
    assert_eq!(engine.stats().await.unwrap().embedded, 0);
    assert_eq!(engine.embed_pending(100).await.unwrap(), 1);
}

struct InvalidEmbedder(Vec<f32>);

#[tokio::test]
async fn only_successfully_committed_chunks_enter_the_cache() {
    let store = Store::in_memory().unwrap();
    let drafts = (0..201)
        .map(|n| ItemDraft::new("journalArticle").with_field("title", format!("Paper {n}")))
        .collect();
    let created = store
        .items
        .create_many(store.default_library, drafts)
        .await
        .unwrap();
    assert!(created.into_iter().all(|r| r.is_ok()));
    let engine = SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap();
    store
        .db()
        .call(|c| {
            c.execute_batch(
                "CREATE TRIGGER fail_second_chunk BEFORE INSERT ON item_vectors
         WHEN (SELECT count(*) FROM item_vectors) >= 200 BEGIN
         SELECT RAISE(ABORT, 'second chunk failed'); END;",
            )
            .map_err(sql_err)
        })
        .await
        .unwrap();
    assert!(engine.embed_pending(201).await.is_err());
    assert_eq!(stored(&store).await, 200);
    assert_eq!(engine.stats().await.unwrap().embedded, 200);
    assert_eq!(queued(&store).await, 1);
}

#[async_trait]
impl EmbeddingProvider for InvalidEmbedder {
    fn id(&self) -> &str {
        "invalid"
    }
    fn dimensions(&self) -> usize {
        2
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| self.0.clone()).collect())
    }
}

#[tokio::test]
async fn malformed_vectors_remain_retryable_instead_of_disappearing_from_the_queue() {
    for vector in [vec![1.0], vec![f32::NAN, 0.0], vec![f32::INFINITY, 0.0]] {
        let store = Store::in_memory().unwrap();
        item(&store, "Invalid reply").await;
        let engine = SearchEngine::new(store.clone(), Arc::new(InvalidEmbedder(vector))).unwrap();
        assert!(engine.embed_pending(100).await.is_err());
        assert_eq!(queued(&store).await, 1);
        assert_eq!(stored(&store).await, 0);
        assert_eq!(engine.stats().await.unwrap().embedded, 0);
    }
}
