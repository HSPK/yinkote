//! Concurrent writers must queue behind each other, not fail.
//!
//! This reproduces a real bug: with deferred transactions SQLite returns
//! `SQLITE_BUSY` immediately when it cannot upgrade a read lock, so the busy
//! timeout never gets a chance to help.

use std::sync::Arc;

use yk_core::model::ItemDraft;
use yk_core::query::ItemFilter;
use yk_store::Store;

const WORKERS: i64 = 8;
const BATCHES: i64 = 5;
const PER_BATCH: i64 = 50;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn parallel_batch_writes_do_not_fail() {
    let dir = std::env::temp_dir().join(format!("yk-conc-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();

    let store = Store::open(Some(&dir.join("test.db"))).unwrap();
    let lib = store.default_library;

    let busy: i64 = store
        .db()
        .call(|c| c.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).map_err(yk_store::sql_err))
        .await
        .unwrap();
    assert!(busy >= 1000, "busy_timeout not applied to pooled connections: {busy}");

    let store = Arc::new(store);
    let mut handles = Vec::new();
    for w in 0..WORKERS {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            for b in 0..BATCHES {
                let drafts: Vec<ItemDraft> = (0..PER_BATCH)
                    .map(|i| {
                        ItemDraft::new("journalArticle")
                            .with_field("title", format!("w{w}-b{b}-{i}"))
                    })
                    .collect();
                store.items.create_many(lib, drafts).await.expect("batch write");
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let total =
        store.items.count(&ItemFilter { library_id: lib, ..Default::default() }).await.unwrap();
    assert_eq!(total, WORKERS * BATCHES * PER_BATCH);

    std::fs::remove_dir_all(&dir).ok();
}
