//! A download wanted for an item that no longer exists is not wanted.
//!
//! `fetch_queue` refers to items by key rather than by a foreign key, so
//! nothing cascaded when an item was deleted. The scratch library had
//! accumulated 54,540 queued downloads, and the worker was still working
//! through DOIs belonging to papers that had been deleted — network spent on
//! rows that could never be attached to anything.

use yk_core::model::ItemDraft;
use yk_store::{DownloadDraft, Store};

async fn queued(store: &Store, lib: i64) -> usize {
    store.downloads.list(lib, 500).await.unwrap().len()
}

#[tokio::test]
async fn deleting_an_item_takes_its_queued_downloads_with_it() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    let doomed = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Going away"))
        .await
        .unwrap();
    let kept = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Staying put"))
        .await
        .unwrap();

    store
        .downloads
        .enqueue(
            lib,
            vec![
                DownloadDraft {
                    item_key: doomed.key.to_string(),
                    url: "https://example.org/doomed.pdf".into(),
                    title: "Going away".into(),
                },
                DownloadDraft {
                    item_key: kept.key.to_string(),
                    url: "https://example.org/kept.pdf".into(),
                    title: "Staying put".into(),
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(queued(&store, lib).await, 2);

    store.items.delete(lib, std::slice::from_ref(&doomed.key)).await.unwrap();

    let left = store.downloads.list(lib, 500).await.unwrap();
    assert_eq!(left.len(), 1, "a deleted item's download was left in the queue");
    assert_eq!(left[0].item_key, kept.key.to_string(), "the wrong one was removed");
}
