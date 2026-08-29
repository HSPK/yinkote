//! What the assistant may look at.
//!
//! Split from the composition root for the same reason `actions` was: a file
//! per kind of power, so "what can this thing see" and "what can this thing
//! change" are each answerable by reading one list rather than by scanning a
//! module that also wires up a provider.
//!
//! Nothing here writes.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use yk_ai::{Tool, ToolSpec};
use yk_core::ports::SearchIndex;
use yk_core::query::{ItemFilter, SearchMode, SearchRequest};
use yk_core::{Error, Result};
use yk_store::Store;

/// How much of an abstract to show a model. Enough to judge relevance, short
/// enough that ten results still fit in a modest context.
const ABSTRACT_CHARS: usize = 400;

pub const SYSTEM_PROMPT: &str = "\
You are a research assistant working inside the user's personal reference \
library. Answer questions using the tools to look things up; never invent a \
citation, a title or an author. When you refer to an item, give its title and \
its key so the user can find it. If the library has nothing relevant, say so \
plainly instead of answering from memory. Be concise.

What makes this library worth asking is not its catalogue — any database has \
one — but what the user put into it. When a question is about a paper's \
content or about what they made of it, use read_paper rather than get_item: \
it carries their notes and the passages they highlighted. When a question is \
about what a paper stands on, use list_references, which also says which of \
those the library already holds.

You can also change the library: add, edit, tag, file and remove items. Do what \
the user asks without asking permission for ordinary edits, but say afterwards \
what you changed. Two habits matter. When you have a DOI, arXiv id or URL, use \
quick_add rather than writing the fields yourself — the publisher's metadata is \
better than your memory of it. And when removing something, use trash_items: it \
is what the user can undo. Only delete permanently if they say so.

You have a workspace directory of your own for notes, drafts and results that \
should outlive a message. Use write_file when a result is a table or a list \
worth keeping, and say where you put it.";

/// Cut a string without splitting a character in half.
pub fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// The fields worth spending context on, and no more.
pub fn summarise(item: &yk_core::model::Item) -> Value {
    json!({
        "key": item.key.as_str(),
        "title": item.title(),
        "itemType": item.item_type,
        "creators": item.creators.iter().map(|c| c.display()).collect::<Vec<_>>(),
        "date": item.field("date").unwrap_or_default(),
        "publication": item.field("publicationTitle").unwrap_or_default(),
        "tags": item.tags.iter().map(|t| t.tag.clone()).collect::<Vec<_>>(),
        "abstract": truncate(item.field("abstractNote").unwrap_or_default(), ABSTRACT_CHARS),
    })
}

pub struct SearchLibrary {
    pub store: Store,
    pub search: Arc<dyn SearchIndex>,
}

#[async_trait]
impl Tool for SearchLibrary {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_library".into(),
            description: "Search the user's library. Supports the same operators as the search \
                 box: tag:x, -tag:x, type:book, author:name, year:2020..2024, \"exact phrase\"."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to look for." },
                    "limit": {
                        "type": "integer",
                        "description": "How many results, 1-20. Defaults to 8.",
                    },
                    "collection": {
                        "type": "string",
                        "description": "Restrict the search to one collection, by key.",
                    },
                },
                "required": ["query"],
            }),
        }
    }

    async fn call(&self, library_id: i64, arguments: Value) -> Result<Value> {
        let query = yk_agent::required_str(&arguments, "query")?;
        let limit = arguments["limit"].as_u64().unwrap_or(8).clamp(1, 20) as u32;
        // A wrong key would otherwise silently widen the search back to the
        // whole library, which looks like the filter working and finding more.
        let collection = match arguments["collection"].as_str().filter(|s| !s.is_empty()) {
            Some(raw) => Some(
                raw.parse()
                    .map_err(|_| Error::invalid(format!("'{raw}' is not a collection key")))?,
            ),
            None => None,
        };

        let hits = self
            .search
            .search(&SearchRequest {
                text: query,
                // Hybrid because the agent's queries are prose, not operators;
                // it has no way to know which retrieval mode suits its question.
                mode: SearchMode::Hybrid,
                filter: ItemFilter {
                    library_id,
                    collection,
                    // Sub-collections count: a user who scopes a chat to
                    // "Diffusion" means the pile, not just its top level.
                    recursive: true,
                    ..Default::default()
                },
                limit,
                offset: 0,
                highlight: false,
            })
            .await?;

        let hits = hits.hits;
        let keys: Vec<_> = hits.iter().map(|h| h.key.clone()).collect();
        let items = self.store.items.get_many(library_id, &keys).await?;
        Ok(json!({
            "count": items.len(),
            "results": items.iter().map(summarise).collect::<Vec<_>>(),
        }))
    }
}

pub struct GetItem {
    pub store: Store,
}

#[async_trait]
impl Tool for GetItem {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "get_item".into(),
            description: "Fetch one item's full metadata by its key.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "key": { "type": "string" } },
                "required": ["key"],
            }),
        }
    }

    async fn call(&self, library_id: i64, arguments: Value) -> Result<Value> {
        let raw = yk_agent::required_str(&arguments, "key")?;
        let key = raw.parse().map_err(|_| Error::invalid(format!("'{raw}' is not an item key")))?;
        let item = self.store.items.get(library_id, &key).await?;
        Ok(serde_json::to_value(item).map_err(|e| Error::internal(e.to_string()))?)
    }
}

/// Everything this library knows about one paper.
///
/// `get_item` returns the catalogue record, which is what any catalogue would
/// hold. What makes a personal library worth asking is the rest: the notes
/// somebody wrote, the passages they highlighted, and what the paper stands
/// on. An assistant that can only see the abstract is answering from the same
/// information as a search engine.
pub struct ReadPaper {
    pub store: Store,
}

/// How much of a note or a highlight to include.
///
/// Long enough to carry an argument, short enough that a paper with two
/// hundred highlights does not fill the context on its own.
const EXCERPT_CHARS: usize = 600;

/// The most notes or highlights to include from one paper.
const MAX_EXCERPTS: usize = 40;

#[async_trait]
impl Tool for ReadPaper {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_paper".into(),
            description: "Everything the library holds about one paper: its metadata, the \
                 user's own notes and highlights, its attachments, and what it cites. Use this \
                 rather than get_item when the question is about the paper's content or about \
                 what the user thought of it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "key": { "type": "string" } },
                "required": ["key"],
            }),
        }
    }

    async fn call(&self, library_id: i64, arguments: Value) -> Result<Value> {
        let raw = yk_agent::required_str(&arguments, "key")?;
        let key: yk_core::Key =
            raw.parse().map_err(|_| Error::invalid(format!("'{raw}' is not an item key")))?;
        let item = self.store.items.get(library_id, &key).await?;
        let children = self.store.items.children(library_id, &key).await.unwrap_or_default();

        // An annotation hangs off the attachment it was drawn on, not off the
        // paper, so the highlights are a level deeper than the notes.
        let mut notes = Vec::new();
        let mut highlights = Vec::new();
        let mut files = Vec::new();
        for child in &children {
            match child.item_type.as_str() {
                "note" => notes.push(json!({
                    "title": child.title(),
                    "text": truncate(child.field("note").unwrap_or_default(), EXCERPT_CHARS),
                })),
                "attachment" => {
                    files.push(json!({
                        "key": child.key.as_str(),
                        "filename": child.field("filename").unwrap_or_default(),
                        "url": child.field("url").unwrap_or_default(),
                    }));
                    for a in self
                        .store
                        .items
                        .children(library_id, &child.key)
                        .await
                        .unwrap_or_default()
                    {
                        if a.item_type == "annotation" {
                            highlights.push(json!({
                                "page": a.field("annotationPage").unwrap_or_default(),
                                "text": truncate(
                                    a.field("annotationText").unwrap_or_default(),
                                    EXCERPT_CHARS,
                                ),
                                "comment": truncate(
                                    a.field("annotationComment").unwrap_or_default(),
                                    EXCERPT_CHARS,
                                ),
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
        notes.truncate(MAX_EXCERPTS);
        highlights.truncate(MAX_EXCERPTS);

        let cites = self.store.relations.cites(library_id, &key).await.unwrap_or_default();
        Ok(json!({
            "item": summarise(&item),
            "notes": notes,
            "highlights": highlights,
            "files": files,
            // Counted rather than listed in full: a bibliography is a hundred
            // lines that rarely answer the question being asked, and
            // `list_references` is there when it does.
            "referenceCount": cites.len(),
            "referencesHeld": cites.iter().filter(|c| c.key.is_some()).count(),
        }))
    }
}

/// A paper's bibliography, and which of it the library holds.
///
/// `read_paper` counts the references because a hundred lines rarely answer
/// the question being asked. When they do — "what does this paper lean on
/// that I have not read?" — this is the tool, and the answer is a list the
/// library can act on rather than a number.
pub struct ListReferences {
    pub store: Store,
}

/// The most references to hand over at once.
///
/// A review article can cite four hundred works. Past this the answer is not
/// a bibliography, it is a context window.
const MAX_REFERENCES: usize = 120;

#[async_trait]
impl Tool for ListReferences {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_references".into(),
            description: "The works a paper cites, in the order it cites them, saying which \
                 ones this library already holds. Use it to answer what a paper stands on, or \
                 to find what it cites that the user has not read."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "held": {
                        "type": "string",
                        "enum": ["all", "yes", "no"],
                        "description":
                            "Filter to references the library holds (yes), does not hold (no), \
                             or everything (all, the default).",
                    },
                },
                "required": ["key"],
            }),
        }
    }

    async fn call(&self, library_id: i64, arguments: Value) -> Result<Value> {
        let raw = yk_agent::required_str(&arguments, "key")?;
        let key: yk_core::Key =
            raw.parse().map_err(|_| Error::invalid(format!("'{raw}' is not an item key")))?;
        let held = arguments["held"].as_str().unwrap_or("all");

        let all = self.store.relations.cites(library_id, &key).await?;
        let total = all.len();
        let kept: Vec<_> = all
            .into_iter()
            .filter(|c| match held {
                "yes" => c.key.is_some(),
                "no" => c.key.is_none(),
                _ => true,
            })
            .take(MAX_REFERENCES)
            .map(|c| {
                json!({
                    "position": c.position + 1,
                    // The key when the library holds it, so the model can go
                    // straight to the paper rather than searching for a title.
                    "key": c.key.map(|k| k.to_string()),
                    "label": c.label,
                    "year": c.year,
                    "doi": c.doi,
                })
            })
            .collect();

        Ok(json!({
            "total": total,
            "returned": kept.len(),
            // Said outright, because a model handed 120 of 400 references has
            // no way to tell that it is looking at part of a list.
            "truncated": kept.len() < total && held == "all",
            "references": kept,
        }))
    }
}

pub struct LibraryOverview {
    pub store: Store,
}

#[async_trait]
impl Tool for LibraryOverview {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "library_overview".into(),
            description: "How many items the library holds, its collections and its most-used \
                 tags. Useful for orienting before searching, or for answering questions about \
                 the library's size and organisation."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, library_id: i64, _arguments: Value) -> Result<Value> {
        let filter = ItemFilter { library_id, ..Default::default() };
        let collections = self.store.collections.list(library_id).await?;
        let tags = self.store.tags.facets(&filter, 40).await?;

        // State the total outright. Left to infer it, a model sums the
        // collection counts — which double-counts anything filed twice and
        // misses anything filed nowhere, and is wrong in a way that looks right.
        let total = self
            .store
            .items
            .list(&yk_core::query::ItemQuery { filter, limit: 1, ..Default::default() })
            .await?
            .total;

        // Capped like the tags beside them. Unbounded, this listed all 588
        // collections of a real library, and the answer that came back was
        // empty: an overview big enough to crowd out the question is not an
        // overview. The biggest are the ones worth naming.
        const SHOWN: usize = 40;
        let mut biggest: Vec<_> = collections.iter().collect();
        biggest.sort_by_key(|c| std::cmp::Reverse(c.item_count));
        let omitted = biggest.len().saturating_sub(SHOWN);

        Ok(json!({
            "itemCount": total,
            "collections": biggest
                .iter()
                .take(SHOWN)
                .map(|c| json!({ "name": c.name, "items": c.item_count }))
                .collect::<Vec<_>>(),
            // Said outright, so the model does not read a truncated list as
            // the whole library.
            "collectionsOmitted": omitted,
            "collectionCount": collections.len(),
            "tags": tags
                .iter()
                .map(|t| json!({ "tag": t.name, "items": t.count }))
                .collect::<Vec<_>>(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // Slicing by byte would panic mid-character on any CJK abstract.
        assert_eq!(truncate("扩散模型综述", 3), "扩散模…");
        assert_eq!(truncate("short", 40), "short");
    }
}

#[cfg(test)]
mod read_paper_tests {
    use super::*;
    use yk_core::model::ItemDraft;
    use yk_store::Store;

    /// A paper with the things a personal library actually adds to it.
    async fn library() -> (Store, i64, yk_core::Key) {
        let store = Store::in_memory().unwrap();
        let lib = store.default_library;

        let paper = store
            .items
            .create(
                lib,
                ItemDraft::new("journalArticle")
                    .with_field("title", "Attention Is All You Need")
                    .with_field("abstractNote", "A new architecture."),
            )
            .await
            .unwrap();

        let mut note = ItemDraft::new("note")
            .with_field("note", "<p>The ablations are the interesting part.</p>");
        note.parent_key = Some(paper.key.clone());
        store.items.create(lib, note).await.unwrap();

        let mut file = ItemDraft::new("attachment").with_field("filename", "paper.pdf");
        file.parent_key = Some(paper.key.clone());
        let file = store.items.create(lib, file).await.unwrap();

        let mut mark = ItemDraft::new("annotation")
            .with_field("annotationText", "scaled dot-product attention")
            .with_field("annotationComment", "why the scaling?")
            .with_field("annotationPage", "4");
        mark.parent_key = Some(file.key.clone());
        store.items.create(lib, mark).await.unwrap();

        (store, lib, paper.key)
    }

    #[tokio::test]
    async fn hands_over_what_only_this_library_knows() {
        let (store, lib, key) = library().await;
        let out = ReadPaper { store }.call(lib, json!({ "key": key.as_str() })).await.unwrap();

        // The catalogue record is what any catalogue holds; the notes and the
        // highlights are why this library is worth asking.
        assert_eq!(out["item"]["title"], "Attention Is All You Need");
        assert!(out["notes"][0]["text"].as_str().unwrap().contains("ablations"));
        assert!(out["highlights"][0]["text"].as_str().unwrap().contains("dot-product"));
        assert_eq!(out["highlights"][0]["comment"], "why the scaling?");
        assert_eq!(out["highlights"][0]["page"], "4");
    }

    #[tokio::test]
    async fn finds_highlights_a_level_deeper_than_the_notes() {
        let (store, lib, key) = library().await;
        let out = ReadPaper { store }.call(lib, json!({ "key": key.as_str() })).await.unwrap();

        // An annotation hangs off the attachment it was drawn on, not off the
        // paper. Looking only at the paper's own children finds none of them.
        assert_eq!(out["files"][0]["filename"], "paper.pdf");
        assert_eq!(out["highlights"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_paper_with_nothing_attached_says_so_rather_than_failing() {
        let store = Store::in_memory().unwrap();
        let lib = store.default_library;
        let bare = store.items.create(lib, ItemDraft::new("book")).await.unwrap();

        let out = ReadPaper { store }.call(lib, json!({ "key": bare.key.as_str() })).await.unwrap();
        assert_eq!(out["notes"].as_array().unwrap().len(), 0);
        assert_eq!(out["highlights"].as_array().unwrap().len(), 0);
        assert_eq!(out["referenceCount"], 0);
    }
}

#[cfg(test)]
mod list_references_tests {
    use super::*;
    use yk_core::model::ItemDraft;
    use yk_store::relations::CitationDraft;
    use yk_store::Store;

    fn reference(doi: &str, label: &str) -> CitationDraft {
        CitationDraft {
            // Normalised, the one way every fingerprint in the program is
            // made — a raw DOI here resolves to nothing.
            fingerprint: format!("doi:{}", yk_core::text::normalize(doi)),
            doi: doi.into(),
            label: label.into(),
            year: Some(2018),
        }
    }

    async fn paper_citing_three() -> (Store, i64, yk_core::Key) {
        let store = Store::in_memory().unwrap();
        let lib = store.default_library;

        // One of the three is on the shelf.
        store
            .items
            .create(
                lib,
                ItemDraft::new("journalArticle")
                    .with_field("title", "On the shelf")
                    .with_field("DOI", "10.1/held"),
            )
            .await
            .unwrap();

        let paper = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        store
            .relations
            .set_citations(
                lib,
                &paper.key,
                vec![
                    reference("10.1/held", "On the shelf"),
                    reference("10.1/absent", "Not here"),
                    reference("10.1/other", "Also not here"),
                ],
            )
            .await
            .unwrap();

        (store, lib, paper.key)
    }

    #[tokio::test]
    async fn lists_a_bibliography_in_the_order_it_was_printed() {
        let (store, lib, key) = paper_citing_three().await;
        let out =
            ListReferences { store }.call(lib, json!({ "key": key.as_str() })).await.unwrap();

        assert_eq!(out["total"], 3);
        assert_eq!(out["references"][0]["position"], 1);
        assert_eq!(out["references"][0]["label"], "On the shelf");
        // Renumbering somebody's bibliography is quiet damage.
        assert_eq!(out["references"][2]["position"], 3);
    }

    #[tokio::test]
    async fn says_which_ones_the_library_holds() {
        let (store, lib, key) = paper_citing_three().await;
        let out =
            ListReferences { store }.call(lib, json!({ "key": key.as_str() })).await.unwrap();

        // The key, not just a flag: the model can go straight to the paper
        // instead of searching for a title it has just been given.
        assert!(out["references"][0]["key"].is_string());
        assert!(out["references"][1]["key"].is_null());
    }

    #[tokio::test]
    async fn narrows_to_what_has_not_been_read() {
        let (store, lib, key) = paper_citing_three().await;
        let out = ListReferences { store }
            .call(lib, json!({ "key": key.as_str(), "held": "no" }))
            .await
            .unwrap();

        // "What does this lean on that I have not read" is the question a
        // bibliography is usually opened for.
        assert_eq!(out["references"].as_array().unwrap().len(), 2);
        assert!(out["references"].as_array().unwrap().iter().all(|r| r["key"].is_null()));
        // The total still describes the paper, not the filter.
        assert_eq!(out["total"], 3);
    }
}

