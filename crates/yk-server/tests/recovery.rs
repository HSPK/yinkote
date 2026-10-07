use serde_json::json;
use yk_ai::Tool;
use yk_core::model::ItemDraft;
use yk_server::{
    agent::{Action, LibraryAction},
    config::Config,
    tasks::{Phase, Tasks},
};
use yk_store::{DownloadDraft, Store};

#[tokio::test]
async fn restart_preserves_results_and_marks_running_work_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let store = Store::open(Some(&path)).unwrap();
    let tasks = Tasks::open(store.db().clone()).await.unwrap();
    let done = tasks.start("export", "task.packing").await.unwrap();
    let running = tasks.start("import", "task.readingArchive").await.unwrap();
    tasks
        .finish(&done, json!({ "file": "library.yinkote" }))
        .await;
    let done_id = done.snapshot().id;
    let running_id = running.snapshot().id;
    drop(tasks);
    drop(store);

    let store = Store::open(Some(&path)).unwrap();
    let restored = Tasks::open(store.db().clone()).await.unwrap();
    assert_eq!(
        restored.get(&done_id).unwrap().result.unwrap()["file"],
        "library.yinkote"
    );
    let interrupted = restored.get(&running_id).unwrap();
    assert_eq!(interrupted.phase, Phase::Failed);
    assert!(interrupted.error.unwrap().contains("server stopped"));
    assert!(!restored.bulk_write_running());
    let next = restored
        .start("import", "task.readingArchive")
        .await
        .unwrap();
    assert_ne!(next.snapshot().id, running_id);
}

#[tokio::test]
async fn interrupted_downloads_become_actionable_failures_not_duplicate_transfers() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    store
        .downloads
        .enqueue(
            lib,
            vec![DownloadDraft {
                item_key: "AAAA1111".into(),
                url: "https://example.test/paper.pdf".into(),
                title: "Paper".into(),
            }],
        )
        .await
        .unwrap();
    let claimed = store.downloads.claim(lib).await.unwrap().unwrap();
    assert_eq!(store.downloads.recover_interrupted().await.unwrap(), 1);
    let job = store.downloads.list(lib, 10).await.unwrap().remove(0);
    assert_eq!(job.state, "failed");
    assert!(job.error.contains("server stopped"));
    assert_eq!(job.attempts, 1);
    store.downloads.retry(lib, &[claimed.id]).await.unwrap();
    assert!(store.downloads.claim(lib).await.unwrap().is_some());
}

#[tokio::test]
async fn agent_deletion_queues_descendant_files_and_emits_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let app = yk_server::build_with_store(
        Config {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        },
        Store::in_memory().unwrap(),
    )
    .await
    .unwrap();
    let lib = app.services.default_library;
    let parent = app
        .store()
        .items
        .create(
            lib,
            ItemDraft::new("journalArticle").with_field("title", "Paper"),
        )
        .await
        .unwrap();
    let child = app
        .store()
        .items
        .create(
            lib,
            ItemDraft {
                parent_key: Some(parent.key.clone()),
                ..ItemDraft::new("attachment").with_field("title", "PDF")
            },
        )
        .await
        .unwrap();
    app.storage()
        .put(&child.key, "paper.pdf", b"pdf")
        .await
        .unwrap();
    let mut events = app.events().subscribe();
    let action = LibraryAction {
        action: Action::DeleteItems,
        store: app.store().clone(),
        scrape: app.scrape().clone(),
        search: app.outside().clone(),
        events: app.events().clone(),
    };
    action
        .call(lib, json!({ "keys": [parent.key.as_str()] }))
        .await
        .unwrap();
    assert!(events.try_recv().is_ok());
    assert!(app.storage().get(&child.key, "paper.pdf").await.is_ok());
    assert_eq!(
        app.store()
            .db()
            .pending_file_cleanup(200)
            .await
            .unwrap()
            .len(),
        2
    );
    yk_server::deletion::cleanup(&app).await.unwrap();
    assert!(app.storage().get(&child.key, "paper.pdf").await.is_err());
    assert!(app
        .store()
        .db()
        .pending_file_cleanup(200)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_rolled_back_deletion_never_schedules_file_removal() {
    let store = Store::in_memory().unwrap();
    let item = store
        .items
        .create(
            store.default_library,
            ItemDraft::new("journalArticle").with_field("title", "Keep"),
        )
        .await
        .unwrap();
    let key = item.key.clone();
    store
        .db()
        .call(move |c| {
            let tx = c.transaction().map_err(yk_store::sql_err)?;
            tx.execute("DELETE FROM items WHERE key=?1", [key.as_str()])
                .map_err(yk_store::sql_err)?;
            tx.rollback().map_err(yk_store::sql_err)
        })
        .await
        .unwrap();
    assert!(store
        .db()
        .pending_file_cleanup(200)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .items
        .get(store.default_library, &item.key)
        .await
        .is_ok());
}

#[tokio::test]
async fn persisted_history_is_bounded_and_keeps_the_latest_result() {
    let store = Store::in_memory().unwrap();
    let tasks = Tasks::open(store.db().clone()).await.unwrap();
    let mut latest = String::new();
    for index in 0..55 {
        let task = tasks.start("export", "task.packing").await.unwrap();
        latest = task.snapshot().id;
        tasks.finish(&task, json!({ "index": index })).await;
    }
    let reopened = Tasks::open(store.db().clone()).await.unwrap();
    assert_eq!(reopened.list().len(), 50);
    assert_eq!(reopened.get(&latest).unwrap().result.unwrap()["index"], 54);
}

#[tokio::test]
async fn a_journal_failure_is_not_reported_as_a_completed_task() {
    let store = Store::in_memory().unwrap();
    let tasks = Tasks::open(store.db().clone()).await.unwrap();
    let task = tasks.start("export", "task.packing").await.unwrap();
    store
        .db()
        .call(|c| {
            c.execute_batch("DROP TABLE task_records")
                .map_err(yk_store::sql_err)
        })
        .await
        .unwrap();
    tasks
        .finish(&task, json!({ "file": "already-written.yinkote" }))
        .await;
    let state = task.snapshot();
    assert_eq!(state.phase, Phase::Failed);
    assert!(state.error.unwrap().contains("persist task completion"));
    assert_eq!(state.result.unwrap()["file"], "already-written.yinkote");
    assert!(tasks.start("export", "task.packing").await.is_err());
}
