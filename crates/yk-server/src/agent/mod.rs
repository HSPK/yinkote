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
use yk_core::ports::{
    ChatMessage, ChatProvider, ChatRequest, SearchIndex, Tool, ToolCall, ToolSpec,
};
use yk_core::query::{ItemFilter, SearchMode, SearchRequest};
use yk_core::{Error, Result};
use yk_store::Store;

use crate::config::AgentConfig;

pub mod actions;
pub use actions::{Action, LibraryAction, ACTIONS};

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
is what the user can undo. Only delete permanently if they say so.";

// ─── provider ───────────────────────────────────────────────────────────────

pub struct OpenAiProvider {
    http: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiProvider {
    pub fn new(config: &AgentConfig) -> Result<Self> {
        let endpoint = config
            .endpoint
            .clone()
            .ok_or_else(|| Error::invalid("agent.endpoint is not configured"))?;
        let model = config
            .model
            .clone()
            .ok_or_else(|| Error::invalid("agent.model is not configured"))?;
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(config.timeout_secs))
                .build()
                .map_err(|e| Error::internal(e.to_string()))?,
            endpoint: format!("{}/chat/completions", endpoint.trim_end_matches('/')),
            model,
            api_key: config.api_key.clone(),
        })
    }
}

/// Our message shape, in the wire's terms.
fn to_wire(message: &ChatMessage) -> Value {
    let mut out = json!({ "role": message.role, "content": message.content });
    if !message.tool_calls.is_empty() {
        out["tool_calls"] = Value::Array(
            message
                .tool_calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": { "name": c.name, "arguments": c.arguments.to_string() },
                    })
                })
                .collect(),
        );
    }
    if let Some(id) = &message.tool_call_id {
        out["tool_call_id"] = json!(id);
    }
    out
}

/// The wire's message shape, in ours.
///
/// Arguments arrive as a JSON *string*, and a model will occasionally produce
/// one that does not parse. Passing an empty object on rather than failing the
/// turn lets the tool report the missing argument, which the model can act on.
fn from_wire(value: &Value) -> ChatMessage {
    let calls = value["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default();
    ChatMessage {
        role: value["role"].as_str().unwrap_or("assistant").to_string(),
        content: value["content"].as_str().unwrap_or_default().to_string(),
        tool_calls: calls
            .iter()
            .map(|c| ToolCall {
                id: c["id"].as_str().unwrap_or_default().to_string(),
                name: c["function"]["name"].as_str().unwrap_or_default().to_string(),
                arguments: c["function"]["arguments"]
                    .as_str()
                    .and_then(|a| serde_json::from_str(a).ok())
                    .unwrap_or_else(|| json!({})),
            })
            .filter(|c| !c.name.is_empty())
            .collect(),
        tool_call_id: None,
        // Providers disagree about where reasoning lives, and none of them
        // agree with the base spec. Try the two spellings in the wild and
        // treat its absence as ordinary — most models expose none.
        reasoning: value["reasoning_content"]
            .as_str()
            .or_else(|| value["reasoning"].as_str())
            .map(str::to_string)
            .filter(|r| !r.trim().is_empty()),
    }
}

#[async_trait]
impl ChatProvider for OpenAiProvider {
    fn model(&self) -> String {
        self.model.clone()
    }

    async fn complete(&self, request: ChatRequest) -> Result<ChatMessage> {
        let body = json!({
            "model": self.model,
            "messages": request.messages.iter().map(to_wire).collect::<Vec<_>>(),
            "tools": request.tools.iter().map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                },
            })).collect::<Vec<_>>(),
        });

        let mut req = self.http.post(&self.endpoint).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.map_err(|e| Error::internal(e.to_string()))?;
        let status = response.status();
        let payload: Value = response.json().await.map_err(|e| Error::internal(e.to_string()))?;

        if !status.is_success() {
            // Providers put the useful part in different places; prefer the
            // message over the raw body so the UI can show something readable.
            let detail = payload["error"]["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| payload.to_string());
            return Err(Error::internal(format!("model returned {status}: {detail}")));
        }

        payload["choices"][0]["message"]
            .as_object()
            .map(|_| from_wire(&payload["choices"][0]["message"]))
            .ok_or_else(|| Error::internal("model returned no choices"))
    }
}

// ─── tools ──────────────────────────────────────────────────────────────────

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
                },
                "required": ["query"],
            }),
        }
    }

    async fn call(&self, library_id: i64, arguments: Value) -> Result<Value> {
        let query = yk_agent::required_str(&arguments, "query")?;
        let limit = arguments["limit"].as_u64().unwrap_or(8).clamp(1, 20) as u32;

        let hits = self
            .search
            .search(&SearchRequest {
                text: query,
                // Hybrid because the agent's queries are prose, not operators;
                // it has no way to know which retrieval mode suits its question.
                mode: SearchMode::Hybrid,
                filter: ItemFilter { library_id, ..Default::default() },
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
    fn wire_round_trips_a_tool_call() {
        let message = ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "search_library".into(),
                arguments: json!({ "query": "diffusion" }),
            }],
            tool_call_id: None,
                    reasoning: None,
        };

        let wire = to_wire(&message);
        assert_eq!(wire["tool_calls"][0]["function"]["name"], "search_library");
        // Arguments go out as a JSON string, which is what the dialect wants.
        assert!(wire["tool_calls"][0]["function"]["arguments"].is_string());

        assert_eq!(from_wire(&wire), message);
    }

    #[test]
    fn unparsable_arguments_become_an_empty_object() {
        // Models do occasionally emit broken JSON here. Losing the turn over it
        // is worse than letting the tool report the missing argument.
        let wire = json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "x",
                "function": { "name": "get_item", "arguments": "{not json" },
            }],
        });
        assert_eq!(from_wire(&wire).tool_calls[0].arguments, json!({}));
    }

    #[test]
    fn a_nameless_tool_call_is_dropped() {
        let wire = json!({
            "role": "assistant",
            "tool_calls": [{ "id": "x", "function": { "arguments": "{}" } }],
        });
        assert!(from_wire(&wire).tool_calls.is_empty());
    }

    #[test]
    fn plain_content_survives_the_round_trip() {
        let message = ChatMessage::new("assistant", "Nothing in the library matches.");
        assert_eq!(from_wire(&to_wire(&message)), message);
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // Slicing by byte would panic mid-character on any CJK abstract.
        assert_eq!(truncate("扩散模型综述", 3), "扩散模…");
        assert_eq!(truncate("short", 40), "short");
    }
}
