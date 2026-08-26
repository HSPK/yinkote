//! Tests for the connector translation layer.
//!
//! What a translator emits is the contract here, so the fixtures are shaped
//! like real connector traffic rather than like this project's own JSON.

use super::*;

fn object(value: serde_json::Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn paper() -> Map<String, Value> {
    object(json!({
        "itemType": "journalArticle",
        "title": "Attention is all you need",
        "publicationTitle": "Advances in Neural Information Processing Systems",
        "date": "2017-06-12",
        "DOI": "10.1000/xyz",
        "abstractNote": "We propose the Transformer.",
        "creators": [
            { "firstName": "Ashish", "lastName": "Vaswani", "creatorType": "author" },
            { "name": "OpenAI", "creatorType": "author" }
        ],
        "tags": [{ "tag": "transformer" }, "attention"],
        "notes": [{ "note": "<p>Read the ablations.</p>" }],
        "attachments": [
            { "title": "Full Text PDF", "url": "https://example.org/p.pdf", "mimeType": "application/pdf" },
            { "title": "Snapshot", "url": "https://example.org/p", "mimeType": "text/html" }
        ],
        "id": 42,
        "uri": "https://example.org/p"
    }))
}

#[test]
fn keeps_every_field_a_translator_sends() {
    let draft = to_draft(&paper());

    assert_eq!(draft.item_type, "journalArticle");
    assert_eq!(draft.fields.get("DOI").and_then(Value::as_str), Some("10.1000/xyz"));
    assert_eq!(
        draft.fields.get("publicationTitle").and_then(Value::as_str),
        Some("Advances in Neural Information Processing Systems")
    );
}

#[test]
fn does_not_store_the_connectors_own_bookkeeping_as_fields() {
    let draft = to_draft(&paper());

    // `id` and `uri` describe the save, not the paper. Stored, they would show
    // up as columns nothing can explain.
    assert!(!draft.fields.contains_key("id"));
    assert!(!draft.fields.contains_key("uri"));
    assert!(!draft.fields.contains_key("creators"));
    assert!(!draft.fields.contains_key("attachments"));
}

#[test]
fn reads_both_shapes_of_name() {
    let draft = to_draft(&paper());

    assert_eq!(draft.creators[0].last_name.as_deref(), Some("Vaswani"));
    // A single-field name is kept whole: splitting it would invent a surname
    // for an institution, and would mangle most CJK names.
    assert_eq!(draft.creators[1].name.as_deref(), Some("OpenAI"));
    assert!(draft.creators[1].last_name.is_none());
}

#[test]
fn reads_both_shapes_of_tag_and_marks_them_automatic() {
    let draft = to_draft(&paper());
    let names: Vec<&str> = draft.tags.iter().map(|t| t.tag.as_str()).collect();

    assert_eq!(names, vec!["transformer", "attention"]);
    // These came from a translator, not from the user. A library that cannot
    // tell the two apart cannot be tidied.
    assert!(draft.tags.iter().all(|t| t.r#type == 1));
}

#[test]
fn takes_the_pdf_and_leaves_the_page_archive() {
    let found = attachments(&paper());

    assert_eq!(found, vec![("Full Text PDF".to_string(), "https://example.org/p.pdf".to_string())]);
}

#[test]
fn takes_the_notes_a_translator_found() {
    assert_eq!(notes(&paper()), vec!["<p>Read the ablations.</p>".to_string()]);
}

#[test]
fn ignores_a_field_that_is_not_a_scalar() {
    // Translators occasionally emit a nested object for a field this has no
    // column for. Stored, it would be a value nothing can display or search.
    let draft = to_draft(&object(json!({
        "itemType": "webpage",
        "title": "A page",
        "extra": { "nested": true }
    })));

    assert!(!draft.fields.contains_key("extra"));
    assert_eq!(draft.fields.get("title").and_then(Value::as_str), Some("A page"));
}

#[test]
fn an_item_with_no_type_is_still_saved() {
    // Losing a save because a translator omitted a field is worse than filing
    // it as the most general thing it could be.
    let draft = to_draft(&object(json!({ "title": "Something" })));
    assert_eq!(draft.item_type, "webpage");
}

#[test]
fn an_empty_tag_is_not_a_tag() {
    let draft = to_draft(&object(json!({ "itemType": "webpage", "tags": ["", "  ", "real"] })));
    assert_eq!(draft.tags.len(), 1);
}
