use std::sync::Arc;

use serde_json::json;
use yk_core::model::{ItemDraft, ItemPatch};
use yk_core::Key;
use yk_store::Store;

fn draft(parent: &Key, body: &str) -> ItemDraft {
    let mut draft = ItemDraft::new("note").with_field("note", body);
    draft.parent_key = Some(parent.clone());
    draft
}

#[tokio::test]
async fn numbered_notes_are_scoped_to_the_paper_and_leave_manual_notes_unchanged() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    let other = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    let manual = store.items.create(lib, draft(&paper.key, "My manual note")).await.unwrap();
    assert_eq!(manual.title(), "My manual note");
    for number in 1..=3 {
        let body = format!("<blockquote>Quote {number}</blockquote><p>Comment</p>");
        let note = store.create_numbered_note(lib, draft(&paper.key, &body)).await.unwrap();
        assert_eq!(note.title(), number.to_string());
        assert_eq!(note.field("note"), Some(body.as_str()));
        assert_eq!(note.parent_key, Some(paper.key.clone()));
        assert_eq!(json!(store.items.get(lib, &note.key).await.unwrap()), json!(note));
    }
    let note = store.create_numbered_note(lib, draft(&other.key, "Other paper")).await.unwrap();
    assert_eq!(note.title(), "1");
    assert_eq!(json!(store.items.get(lib, &manual.key).await.unwrap()), json!(manual));
}

#[tokio::test]
async fn numbered_notes_skip_existing_numeric_titles_without_renaming_them() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    let existing = store
        .items
        .create(lib, draft(&paper.key, "Existing custom note").with_field("title", "7"))
        .await
        .unwrap();
    let numbered = store.create_numbered_note(lib, draft(&paper.key, "New note")).await.unwrap();
    assert_eq!(numbered.title(), "8");
    assert_eq!(json!(store.items.get(lib, &existing.key).await.unwrap()), json!(existing));
}

#[tokio::test]
async fn numbered_notes_preserve_explicit_titles_and_do_not_consume_a_number() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    let custom = store
        .create_numbered_note(lib, draft(&paper.key, "Quoted text").with_field("title", "My title"))
        .await
        .unwrap();
    assert_eq!(custom.title(), "My title");
    let next = store.create_numbered_note(lib, draft(&paper.key, "More quoted text")).await.unwrap();
    assert_eq!(next.title(), "1");
}

#[tokio::test]
async fn numbered_notes_do_not_reuse_a_number_after_renaming_or_deletion() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    let first = store.create_numbered_note(lib, draft(&paper.key, "1")).await.unwrap();
    let changed = store
        .items
        .update(
            lib,
            &first.key,
            ItemPatch {
                fields: Some(
                    [("title".into(), json!("1")), ("note".into(), json!("Edited body"))]
                        .into_iter()
                        .collect(),
                ),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(changed.title(), "1", "explicit titles win over the previous body");
    store
        .items
        .update(
            lib,
            &first.key,
            ItemPatch {
                fields: Some([("title".into(), json!("Renamed"))].into_iter().collect()),
                deleted: Some(true),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let second = store.create_numbered_note(lib, draft(&paper.key, "Next")).await.unwrap();
    assert_eq!(second.title(), "2");
    store.items.delete(lib, &[first.key, second.key]).await.unwrap();
    let third = store.create_numbered_note(lib, draft(&paper.key, "After deletion")).await.unwrap();
    assert_eq!(third.title(), "3");
}

#[tokio::test]
async fn numbered_notes_roll_back_allocation_when_creation_fails() {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    let version = store.libraries.version(lib).await.unwrap();
    assert!(store
        .create_numbered_note(lib, draft(&paper.key, "Bad").with_field("unknownField", "bad"))
        .await
        .is_err());
    assert_eq!(store.libraries.version(lib).await.unwrap(), version);
    let first = store.create_numbered_note(lib, draft(&paper.key, "Good")).await.unwrap();
    assert_eq!(first.title(), "1");
    assert!(store.create_numbered_note(lib, ItemDraft::new("note")).await.is_err());
    assert!(store.create_numbered_note(lib, ItemDraft::new("journalArticle")).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn numbered_notes_allocate_atomically_across_independent_connection_pools() {
    let root = tempfile::Builder::new().prefix(".numbered-notes-").tempdir_in(".").unwrap();
    let path = root.path().join("library.db");
    let store = Store::open(Some(&path)).unwrap();
    let independent = Store::open(Some(&path)).unwrap();
    let lib = store.default_library;
    let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
    const WRITERS: usize = 16;
    let barrier = Arc::new(tokio::sync::Barrier::new(WRITERS));
    let mut handles = Vec::new();
    for index in 0..WRITERS {
        let store = if index % 2 == 0 { store.clone() } else { independent.clone() };
        let parent = paper.key.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            store.create_numbered_note(lib, draft(&parent, &format!("Quote {index}"))).await.unwrap()
        }));
    }
    let mut numbers = Vec::new();
    for handle in handles {
        let note = handle.await.unwrap();
        numbers.push(note.title().parse::<usize>().unwrap());
        assert_eq!(json!(store.items.get(lib, &note.key).await.unwrap()), json!(note));
    }
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=WRITERS).collect::<Vec<_>>());
    drop(independent);
    drop(store);
    let reopened = Store::open(Some(&path)).unwrap();
    let next = reopened.create_numbered_note(lib, draft(&paper.key, "After reopen")).await.unwrap();
    assert_eq!(next.title(), (WRITERS + 1).to_string());
}
