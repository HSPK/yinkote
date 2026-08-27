//! The shapes a conversation is made of.
//!
//! Deliberately one set for every provider. Each service spells these
//! differently on the wire — and the translation is the provider's whole job —
//! but the moment a message reaches this crate's boundary it looks the same
//! regardless of who produced it.

use serde_json::Value;
use yk_core::Result;

/// One turn in a conversation with a model.
#[derive(Clone, Debug, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    /// `system`, `user`, `assistant` or `tool`.
    pub role: String,
    #[serde(default)]
    pub content: String,
    /// Tool calls the assistant asked for.
    #[serde(default, rename = "toolCalls", skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Which call this message answers, when `role` is `tool`.
    #[serde(default, rename = "toolCallId", skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// The model's own reasoning, when it reports it separately from the
    /// answer. Kept apart because it is *not* the answer: it is working, it is
    /// often long, and presenting it as prose would be misleading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self { role: role.into(), content: content.into(), ..Default::default() }
    }
}

/// A tool the model asked to run.
#[derive(Clone, Debug, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Already parsed. A model that emits unparseable arguments gets an empty
    /// object and a tool that complains, which it can act on — rather than an
    /// error that ends the turn.
    #[serde(default)]
    pub arguments: Value,
}

/// What a tool is, as told to the model.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments.
    pub parameters: Value,
}

/// Something the model may do.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, library_id: i64, arguments: Value) -> Result<Value>;
}

/// One request to a model.
#[derive(Clone, Debug, Default)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
}
