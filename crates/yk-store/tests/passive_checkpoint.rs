use std::time::Duration;

use yk_core::model::ItemDraft;
use yk_store::Store;

#[tokio::test]
async fn a_passive_checkpoint_does_not_wait_for_a_foreground_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(Some(&dir.path().join("library.db"))).unwrap();
    store
        .items
        .create(
            store.default_library,
            ItemDraft::new("journalArticle").with_field("title", "Committed"),
        )
        .await
        .unwrap();

    let mut conn = store.db().conn().unwrap();
    let transaction = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transaction
        .execute(
            "UPDATE libraries SET name='Foreground write' WHERE id=1",
            [],
        )
        .unwrap();

    let result =
        tokio::time::timeout(Duration::from_secs(1), store.db().checkpoint(u64::MAX)).await;
    // Release the writer even if the checkpoint unexpectedly waits, so a
    // failing regression does not leave a blocking task behind.
    transaction.commit().unwrap();
    result.expect("PASSIVE waited for the writer").unwrap();
    assert_eq!(
        store
            .libraries
            .get(store.default_library)
            .await
            .unwrap()
            .name,
        "Foreground write"
    );
}
