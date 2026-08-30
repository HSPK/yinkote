//! Housekeeping has to know when somebody is writing.
//!
//! The background workers take the database exclusively — a `TRUNCATE`
//! checkpoint and an index compaction both do — and they used to decide
//! whether that was safe by asking the task registry. That only knows about
//! jobs which announce themselves, so a client writing through ordinary
//! requests was invisible, and the collision came back as `500 database is
//! locked`: an embedding pass, a bulk delete of 87,000 items, and the
//! benchmark's own seeding, which died at 78,000 of 99,939.
//!
//! Writing is observed at the write path instead, so it does not matter who is
//! doing it.

use std::time::Duration;

use yk_core::model::ItemDraft;
use yk_store::{writes_quiet_for, Store};

#[tokio::test]
async fn a_write_is_noticed_by_the_workers() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;

    store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Just written"))
        .await
        .unwrap();

    assert!(
        !writes_quiet_for(Duration::from_secs(5)),
        "a write that just happened was reported as quiet, so a worker would \
         have taken the database while it was being used"
    );

    // And a window that has already elapsed is quiet, which is what lets
    // housekeeping resume the moment a burst ends.
    assert!(
        writes_quiet_for(Duration::from_millis(0)),
        "nothing is ever quiet, so housekeeping would never run at all"
    );
}
