//! Tests for the agent's write tools.
//!
//! These run against a real store, because what matters is what ends up in the
//! library — not whether a JSON blob was shaped correctly on the way.

use super::*;
use std::sync::Arc;
use yk_scrape::ScrapeEngine;
use yk_store::Store;

fn tool(action: Action, store: &Store) -> LibraryAction {
    LibraryAction {
        action,
        store: store.clone(),
        scrape: Arc::new(ScrapeEngine::with_defaults()),
        search: Arc::new(yk_scrape::search::SearchEngine::with_defaults()),
    }
}

async fn library() -> (Store, i64) {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    (store, lib)
}

#[tokio::test]
async fn creates_an_item_from_what_the_model_wrote() {
    let (store, lib) = library().await;
    let out = tool(Action::CreateItems, &store)
        .call(
            lib,
            json!({ "items": [{
                "itemType": "journalArticle",
                "title": "Attention is all you need",
                "DOI": "10.1000/xyz",
                "creators": [{ "creatorType": "author", "lastName": "Vaswani" }],
                "tags": ["transformer"]
            }] }),
        )
        .await
        .unwrap();

    assert_eq!(out["created"], 1);
    let key = out["items"][0]["key"].as_str().unwrap();
    let stored = store.items.get(lib, &yk_core::Key::parse(key).unwrap()).await.unwrap();
    assert_eq!(stored.title(), "Attention is all you need");
    assert_eq!(stored.field("DOI"), Some("10.1000/xyz"));
    assert_eq!(stored.tags[0].tag, "transformer");
    // A tag the agent applied is the user's own: they asked for it in words.
    assert_eq!(stored.tags[0].r#type, 0);
}

#[tokio::test]
async fn refuses_an_item_with_no_title() {
    let (store, lib) = library().await;
    // A model told to look things up sometimes invents one anyway, and an
    // untitled item is the shape that mistake takes.
    let err = tool(Action::CreateItems, &store)
        .call(lib, json!({ "items": [{ "itemType": "book" }] }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("title"), "{err}");
}

#[tokio::test]
async fn edits_only_the_fields_it_was_given() {
    let (store, lib) = library().await;
    let item = store
        .items
        .create(
            lib,
            yk_core::model::ItemDraft::new("book")
                .with_field("title", "Original")
                .with_field("publisher", "Someone"),
        )
        .await
        .unwrap();

    tool(Action::UpdateItem, &store)
        .call(lib, json!({ "key": item.key.as_str(), "fields": { "title": "Corrected" } }))
        .await
        .unwrap();

    let after = store.items.get(lib, &item.key).await.unwrap();
    assert_eq!(after.title(), "Corrected");
    assert_eq!(after.field("publisher"), Some("Someone"), "the rest is left alone");
}

#[tokio::test]
async fn trashes_rather_than_destroying() {
    let (store, lib) = library().await;
    let item = store
        .items
        .create(lib, yk_core::model::ItemDraft::new("book").with_field("title", "Doomed"))
        .await
        .unwrap();

    tool(Action::TrashItems, &store)
        .call(lib, json!({ "keys": [item.key.as_str()] }))
        .await
        .unwrap();
    assert!(store.items.get(lib, &item.key).await.unwrap().deleted);

    tool(Action::RestoreItems, &store)
        .call(lib, json!({ "keys": [item.key.as_str()] }))
        .await
        .unwrap();
    // The reversible path has to actually reverse, or the default is a lie.
    assert!(!store.items.get(lib, &item.key).await.unwrap().deleted);
}

#[tokio::test]
async fn files_items_into_a_collection_it_made() {
    let (store, lib) = library().await;
    let item = store
        .items
        .create(lib, yk_core::model::ItemDraft::new("book").with_field("title", "A book"))
        .await
        .unwrap();

    let made = tool(Action::CreateCollection, &store)
        .call(lib, json!({ "name": "To read" }))
        .await
        .unwrap();
    let collection = made["key"].as_str().unwrap();

    tool(Action::FileItems, &store)
        .call(lib, json!({ "collectionKey": collection, "keys": [item.key.as_str()] }))
        .await
        .unwrap();

    let after = store.items.get(lib, &item.key).await.unwrap();
    assert_eq!(after.collections.len(), 1);
}

#[tokio::test]
async fn adds_and_removes_tags_without_touching_the_others() {
    let (store, lib) = library().await;
    let mut draft = yk_core::model::ItemDraft::new("book").with_field("title", "Tagged");
    draft.tags = vec![ItemTag { tag: "keep".into(), r#type: 0 }];
    let item = store.items.create(lib, draft).await.unwrap();

    tool(Action::TagItems, &store)
        .call(lib, json!({ "keys": [item.key.as_str()], "tags": ["added"] }))
        .await
        .unwrap();
    tool(Action::UntagItems, &store)
        .call(lib, json!({ "keys": [item.key.as_str()], "tags": ["keep"] }))
        .await
        .unwrap();

    let after = store.items.get(lib, &item.key).await.unwrap();
    let tags: Vec<&str> = after.tags.iter().map(|t| t.tag.as_str()).collect();
    assert_eq!(tags, vec!["added"]);
}

#[tokio::test]
async fn rejects_a_key_that_is_not_one() {
    let (store, lib) = library().await;
    // The model produces keys from its own text often enough that this is a
    // real path, not a defensive one.
    let err = tool(Action::TrashItems, &store)
        .call(lib, json!({ "keys": ["not a key"] }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not a valid key"), "{err}");
}

#[test]
fn every_action_is_offered_exactly_once() {
    let mut names: Vec<&str> = ACTIONS.iter().map(|a| a.name()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "two actions share a name");
}

#[test]
fn reading_the_library_is_not_marked_as_changing_it() {
    // The transcript marks write steps so a reader can see at a glance which
    // ones only looked. Getting this backwards would be worse than not marking.
    assert!(!Action::ListCollections.writes());
    assert!(!Action::MissingWorks.writes());
    assert!(Action::DeleteItems.writes());
    assert!(Action::CreateItems.writes());
}
