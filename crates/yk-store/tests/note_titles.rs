//! A note is titled by what it says.
//!
//! There is nowhere to type a note's title — the text *is* the note — so every
//! list that shows one derives it. Four call sites did that separately and the
//! two generic paths did not, so a note written by hand was untitled forever
//! and appeared as a blank row in the table, the sidebar and search results.

use yk_core::model::ItemDraft;
use yk_store::Store;

async fn store() -> (Store, i64) {
    let store = Store::in_memory().unwrap();
    let lib = store.default_library;
    (store, lib)
}

#[tokio::test]
async fn a_note_is_titled_by_its_first_line() {
    let (store, lib) = store().await;
    let note = store
        .items
        .create(
            lib,
            ItemDraft::new("note").with_field("note", "Reading plan\n\nStart with section 3."),
        )
        .await
        .unwrap();

    assert_eq!(note.title(), "Reading plan");
}

#[tokio::test]
async fn editing_the_first_line_renames_the_note() {
    let (store, lib) = store().await;
    let note = store
        .items
        .create(lib, ItemDraft::new("note").with_field("note", "Reading plan"))
        .await
        .unwrap();

    let patch = yk_core::model::ItemPatch {
        fields: Some(
            [("note".to_string(), serde_json::json!("Screening notes\n\nRejected four."))]
                .into_iter()
                .collect(),
        ),
        ..Default::default()
    };
    let updated = store.items.update(lib, &note.key, patch, None).await.unwrap();

    assert_eq!(updated.title(), "Screening notes", "the title follows the text");
}

#[tokio::test]
async fn a_title_somebody_typed_is_not_overwritten() {
    // Summaries and close readings set their own title, and a reader can set
    // one too. Deriving unconditionally would take it away on the next edit.
    let (store, lib) = store().await;
    let note = store
        .items
        .create(
            lib,
            ItemDraft::new("note")
                .with_field("note", "Reading plan\n\nStart with section 3.")
                .with_field("title", "Torelli — my working notes"),
        )
        .await
        .unwrap();

    assert_eq!(note.title(), "Torelli — my working notes");

    let patch = yk_core::model::ItemPatch {
        fields: Some(
            [("note".to_string(), serde_json::json!("Reading plan\n\nAlso section 4."))]
                .into_iter()
                .collect(),
        ),
        ..Default::default()
    };
    let updated = store.items.update(lib, &note.key, patch, None).await.unwrap();
    assert_eq!(updated.title(), "Torelli — my working notes", "still theirs");
}

#[tokio::test]
async fn an_empty_note_is_not_given_a_title_made_of_nothing() {
    // A note starts empty — that is what "write a note" opens — and a title of
    // "" is worse than none because it hides that nothing was written.
    let (store, lib) = store().await;
    let note =
        store.items.create(lib, ItemDraft::new("note").with_field("note", "")).await.unwrap();
    assert_eq!(note.title(), "");
}

#[tokio::test]
async fn only_notes_are_retitled() {
    // Every item type has a `title`, and a paper's is its own.
    let (store, lib) = store().await;
    let paper = store
        .items
        .create(
            lib,
            ItemDraft::new("journalArticle")
                .with_field("title", "Attention Is All You Need")
                .with_field("note", "some stray text"),
        )
        .await
        .unwrap();
    assert_eq!(paper.title(), "Attention Is All You Need");
}
