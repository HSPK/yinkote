//! Bounded reading views shared by REST, MCP and conversation context.

use serde_json::{json, Value};
use yk_core::{Key, Result};
use yk_store::Store;

pub const SYSTEM_PROMPT: &str = "\
You are a research assistant inside the user's personal reference library. \
Use the discovered MCP tools to look up facts; never invent titles, authors or \
citations. Name papers by title and item key. If the library has no relevant \
evidence, say so.

Business tools are discovered from the same operations as the workbench's REST \
API. Follow each tool's schema: route values go in path, search filters in query, \
and write payloads in body. For search_library use query.q and query.limit; for \
get_item or read_paper use path.key. The current library is supplied for you.

Use read_paper for the user's notes, highlights and paper context, and \
list_references for bibliography and cited works. Prefer quick_add with body.text \
when adding a DOI, arXiv identifier or URL, rather than inventing metadata. \
Ordinary edits may be made when requested; summarize what changed. Prefer \
trash_items to permanent deletion. Dangerous operations may be disabled by policy.

For tags, read the current item then use update_item with body.tags, preserving \
existing tags; ifVersion can guard against concurrent edits. Use pagination and \
smaller result sets instead of assuming a truncated response was complete.

Workspace and skill tools are separate from library operations. Use write_file \
for results worth keeping and tell the user where you saved them.";

pub fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

pub fn summarise(item: &yk_core::model::Item) -> Value {
    json!({
        "key": item.key.as_str(), "title": item.title(), "itemType": item.item_type,
        "creators": item.creators.iter().map(|c| c.display()).collect::<Vec<_>>(),
        "date": item.field("date").unwrap_or_default(),
        "publication": item.field("publicationTitle").unwrap_or_default(),
        "tags": item.tags.iter().map(|t| t.tag.clone()).collect::<Vec<_>>(),
        "abstract": truncate(item.field("abstractNote").unwrap_or_default(), 400),
    })
}

pub async fn read_paper(store: &Store, library: i64, key: &Key) -> Result<Value> {
    let item = store.items.get(library, key).await?;
    let children = store.items.children(library, key).await?;
    let attachments: Vec<_> = children
        .iter()
        .filter(|item| item.item_type == "attachment")
        .map(|item| item.key.clone())
        .collect();
    let annotations = store.items.children_of(library, &attachments).await?;
    let notes: Vec<_> = children
        .iter()
        .filter(|item| item.item_type == "note")
        .collect();
    let highlights: Vec<_> = annotations
        .iter()
        .filter(|item| item.item_type == "annotation")
        .collect();
    let cites = store.relations.cites(library, key).await?;
    Ok(json!({
        "item": summarise(&item),
        "notes": notes.iter().take(40).map(|item| json!({
            "key":item.key, "title":item.title(), "text":truncate(item.field("note").unwrap_or_default(), 600),
        })).collect::<Vec<_>>(),
        "highlights": highlights.iter().take(40).map(|item| json!({
            "key":item.key, "page":item.field("annotationPage").unwrap_or_default(),
            "text":truncate(item.field("annotationText").unwrap_or_default(), 600),
            "comment":truncate(item.field("annotationComment").unwrap_or_default(), 600),
        })).collect::<Vec<_>>(),
        "files": children.iter().filter(|item| item.item_type == "attachment").map(|item| json!({
            "key":item.key, "filename":item.field("filename").unwrap_or_default(),
            "url":item.field("url").unwrap_or_default(),
        })).collect::<Vec<_>>(),
        "notesOmitted": notes.len().saturating_sub(40),
        "highlightsOmitted": highlights.len().saturating_sub(40),
        "referenceCount":cites.len(),
        "referencesHeld":cites.iter().filter(|c| c.key.is_some()).count(),
    }))
}

pub async fn overview(store: &Store, library: i64) -> Result<Value> {
    let filter = yk_core::query::ItemFilter {
        library_id: library,
        ..Default::default()
    };
    let mut collections = store.collections.list(library).await?;
    let count = collections.len();
    collections.sort_by_key(|collection| std::cmp::Reverse(collection.item_count));
    let tags = store.tags.facets(&filter, 40).await?;
    Ok(json!({
        "itemCount":store.items.count(&filter).await?,
        "collectionCount":count, "collectionsOmitted":count.saturating_sub(40),
        "collections":collections.iter().take(40).map(|collection| json!({
            "key":collection.key,"name":collection.name,"items":collection.item_count,
        })).collect::<Vec<_>>(),
        "tags":tags.iter().map(|tag| json!({"tag":tag.name,"items":tag.count})).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::ItemDraft;

    #[test]
    fn truncation_preserves_unicode() {
        assert_eq!(truncate("扩散模型综述", 3), "扩散模…");
    }

    #[tokio::test]
    async fn reading_includes_notes_and_annotations_under_attachments() {
        let store = Store::in_memory().unwrap();
        let lib = store.default_library;
        let paper = store
            .items
            .create(
                lib,
                ItemDraft::new("journalArticle").with_field("title", "Paper"),
            )
            .await
            .unwrap();
        let note = ItemDraft {
            parent_key: Some(paper.key.clone()),
            ..ItemDraft::new("note").with_field("note", "A personal observation")
        };
        store.items.create(lib, note).await.unwrap();
        let attachment = store
            .items
            .create(
                lib,
                ItemDraft {
                    parent_key: Some(paper.key.clone()),
                    ..ItemDraft::new("attachment")
                },
            )
            .await
            .unwrap();
        store
            .items
            .create(
                lib,
                ItemDraft {
                    parent_key: Some(attachment.key),
                    ..ItemDraft::new("annotation")
                        .with_field("annotationText", "A highlighted passage")
                },
            )
            .await
            .unwrap();
        let view = read_paper(&store, lib, &paper.key).await.unwrap();
        assert_eq!(view["notes"][0]["text"], "A personal observation");
        assert_eq!(view["highlights"][0]["text"], "A highlighted passage");
        assert_eq!(view["referenceCount"], 0);
    }
}
