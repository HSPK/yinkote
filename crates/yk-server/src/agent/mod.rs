//! Agent wiring: a chat provider, the tools it may use, and the endpoint.
//!
//! The provider speaks the OpenAI chat-completions dialect, which is what
//! local runners (Ollama, llama.cpp, vLLM, LM Studio) and hosted services alike
//! expose — so "which model" is a URL and a name in the config file rather than
//! a code change.
//!
//! The read-only tools are here; everything that *changes* the library lives in
//! `actions`, in its own file, so "what can this thing do to my data" is
//! answerable by reading one list.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use yk_ai::{OpenAiConfig, OpenAiProvider, Tool, ToolSpec};
use yk_core::ports::SearchIndex;
use yk_core::query::{ItemFilter, SearchMode, SearchRequest};
use yk_core::{Error, Result};
use yk_store::Store;

use crate::config::AgentConfig;

/// Point a provider at whatever the config names.
///
/// The dialect itself lives in `yk-ai`; this only translates one config shape
/// into another, which is all a composition root should ever do.
pub fn provider(config: &AgentConfig) -> yk_core::Result<OpenAiProvider> {
    OpenAiProvider::new(&OpenAiConfig {
        endpoint: config.endpoint.clone().unwrap_or_default(),
        model: config.model.clone().unwrap_or_default(),
        api_key: config.api_key.clone(),
        timeout_secs: config.timeout_secs,
    })
}

pub mod actions;
pub mod skills;
pub mod workspace;
pub use actions::{Action, LibraryAction, ACTIONS};
pub use workspace::Workspace;

/// How much of an abstract to show a model. Enough to judge relevance, short
/// enough that ten results still fit in a modest context.
const ABSTRACT_CHARS: usize = 400;

pub const SYSTEM_PROMPT: &str = "\
You are a research assistant working inside the user's personal reference \
library. Answer questions using the tools to look things up; never invent a \
citation, a title or an author. When you refer to an item, give its title and \
its key so the user can find it. If the library has nothing relevant, say so \
plainly instead of answering from memory. Be concise.

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
fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// The fields worth spending context on, and no more.
pub(crate) fn summarise(item: &yk_core::model::Item) -> Value {
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

        Ok(json!({
            "itemCount": total,
            "collections": collections
                .iter()
                .map(|c| json!({ "name": c.name, "items": c.item_count }))
                .collect::<Vec<_>>(),
            "tags": tags
                .iter()
                .map(|t| json!({ "tag": t.name, "items": t.count }))
                .collect::<Vec<_>>(),
        }))
    }
}

/// Everything the agent may do, in one place.
pub fn tools(
    store: &Store,
    search: &Arc<dyn SearchIndex>,
    scrape: &Arc<yk_scrape::ScrapeEngine>,
) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(SearchLibrary { store: store.clone(), search: search.clone() }),
        Arc::new(GetItem { store: store.clone() }),
        Arc::new(LibraryOverview { store: store.clone() }),
    ];
    tools.extend(ACTIONS.iter().map(|action| {
        Arc::new(LibraryAction {
            action: *action,
            store: store.clone(),
            scrape: scrape.clone(),
        }) as Arc<dyn Tool>
    }));
    tools
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
