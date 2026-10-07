use std::time::Duration;

use yk_store::{writes_quiet_for, Store};

// Keep the process-wide quiet-period observation isolated from other writers.
#[tokio::test]
async fn empty_queue_polls_are_reads_even_while_another_connection_is_writing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(Some(&dir.path().join("library.db"))).unwrap();
    assert!(writes_quiet_for(Duration::from_secs(60)));

    let mut conn = store.db().conn().unwrap();
    let transaction = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    // A write poll would wait for this transaction's fifteen-second timeout.
    let polls = tokio::time::timeout(Duration::from_secs(1), async {
        assert!(store
            .downloads
            .claim(store.default_library)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .db()
            .pending_file_cleanup(200)
            .await
            .unwrap()
            .is_empty());
        store.db().finish_file_cleanup(Vec::new()).await.unwrap();
    })
    .await;
    transaction.rollback().unwrap();
    assert!(polls.is_ok(), "an empty queue tried to take the write lock");
    assert!(
        writes_quiet_for(Duration::from_secs(60)),
        "polling reset the quiet timer"
    );
}
