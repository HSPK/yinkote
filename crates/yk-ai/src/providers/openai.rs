//! The OpenAI chat-completions dialect.
//!
//! Which is not only OpenAI's: it is what Ollama, llama.cpp, vLLM, LM Studio,
//! DeepSeek, Qwen and most hosted services expose. Speaking it means "which
//! model" is a URL and a name in a config file rather than a code change.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use yk_core::{Error, Result};

use crate::provider::ChatProvider;
use crate::stream::{Delta, Sink};
use crate::types::{ChatMessage, ChatRequest, ToolCall};

/// What a provider needs to be pointed at a service.
#[derive(Debug, Clone, Default)]
pub struct OpenAiConfig {
    /// A base URL such as `http://127.0.0.1:11434/v1`.
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

pub struct OpenAiProvider {
    http: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiProvider {
    pub fn new(config: &OpenAiConfig) -> Result<Self> {
        if config.endpoint.trim().is_empty() {
            return Err(Error::invalid("no endpoint is configured"));
        }
        if config.model.trim().is_empty() {
            return Err(Error::invalid("no model is configured"));
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(config.timeout_secs.max(1)))
                .build()
                .map_err(|e| Error::internal(e.to_string()))?,
            endpoint: format!("{}/chat/completions", config.endpoint.trim_end_matches('/')),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
        })
    }

    fn body(&self, request: &ChatRequest, streaming: bool) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": request.messages.iter().map(to_wire).collect::<Vec<_>>(),
            "stream": streaming,
        });
        if !request.tools.is_empty() {
            body["tools"] = json!(request
                .tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    },
                }))
                .collect::<Vec<_>>());
        }
        body
    }

    fn send(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut req = self.http.post(&self.endpoint).json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }
}

/// How many times to wait out a busy service before giving up.
///
/// Three, because a limit that has not cleared after three waits is not a
/// blip, and a user staring at a spinner deserves to be told.
const MAX_RETRIES: u32 = 3;

/// The longest a single wait may be, whatever the service asks for.
///
/// A provider that says "retry after 300 seconds" is telling the truth, but
/// nobody is waiting five minutes inside one request.
const MAX_WAIT: Duration = Duration::from_secs(20);

/// Whether a failure is worth waiting out.
///
/// 429 is the common one; 502/503/504 are a proxy or a model still loading,
/// which is the same kind of "try again shortly". A 400 or a 401 will fail
/// identically no matter how long anyone waits.
fn is_transient(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

/// What the service asked for, if it asked.
///
/// `Retry-After` is either seconds or an HTTP date; only the seconds form is
/// read, because the date form needs a clock both ends agree on and is not
/// what these APIs send.
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)
        .or_else(|| headers.get("x-ratelimit-reset-requests"))?
        .to_str()
        .ok()?;
    let seconds: f64 = raw.trim().trim_end_matches('s').parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    // A service that says "0 seconds" means "now", not "spin".
    Some(Duration::from_millis(((seconds * 1000.0) as u64).max(200)).min(MAX_WAIT))
}

/// Doubling, for a service that did not say.
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64 << attempt.min(5)).min(MAX_WAIT)
}

#[async_trait]
impl ChatProvider for OpenAiProvider {
    fn model(&self) -> String {
        self.model.clone()
    }

    /// Streamed, always.
    ///
    /// Worth knowing when this looks broken: **a service may buffer**. The
    /// endpoint used while developing this emits perfectly well-formed SSE and
    /// sends all two hundred events within fifty milliseconds *of each other*,
    /// seven seconds after the request — it computes the whole answer and then
    /// pretends to stream it. There is nothing to fix on this side, and the
    /// only way to tell is to time the arrivals rather than read the format.
    async fn complete(&self, request: ChatRequest, on_delta: Sink<'_>) -> Result<ChatMessage> {
        let body = self.body(&request, true);

        // Rate limits are routine and self-healing — the service usually says
        // how long to wait — so failing the whole turn on one is throwing away
        // work for a condition that resolves itself in a second. Retried here
        // rather than by the caller, because only this layer can read
        // `Retry-After`, and because every caller would otherwise need the
        // same loop.
        let mut attempt = 0u32;
        let response = loop {
            let sent = self.send(&body).send().await.map_err(|e| Error::internal(e.to_string()))?;
            let status = sent.status();
            if status.is_success() {
                break sent;
            }

            if attempt < MAX_RETRIES && is_transient(status.as_u16()) {
                let wait = retry_after(sent.headers()).unwrap_or_else(|| backoff(attempt));
                tracing::warn!(
                    status = status.as_u16(),
                    attempt = attempt + 1,
                    wait_ms = wait.as_millis() as u64,
                    "model busy; retrying"
                );
                tokio::time::sleep(wait).await;
                attempt += 1;
                continue;
            }

            let payload: Value = sent.json().await.unwrap_or_default();
            // Providers put the useful part in different places; prefer the
            // message over the raw body so the UI can show something readable.
            let detail = payload["error"]["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| payload.to_string());
            return Err(Error::internal(format!("model returned {status}: {detail}")));
        };

        let mut assembled = Assembled::default();
        let mut buffer = String::new();
        let mut body = response.bytes_stream();

        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|e| Error::internal(e.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Events are separated by a blank line, and a chunk boundary can
            // fall anywhere — including mid-character. Anything after the last
            // separator is an incomplete event and waits for more bytes.
            while let Some(cut) = buffer.find("\n\n") {
                let event: String = buffer.drain(..cut + 2).collect();
                for line in event.lines() {
                    let Some(data) = line.strip_prefix("data:") else { continue };
                    let data = data.trim();
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }
                    match serde_json::from_str::<Value>(data) {
                        Ok(value) => assembled.absorb(&value, on_delta),
                        // A malformed event is not worth losing the turn over;
                        // the ones around it still carry the answer.
                        Err(e) => tracing::debug!(error = %e, "unreadable stream event"),
                    }
                }
            }
        }

        Ok(assembled.finish())
    }
}

/// A message being built out of fragments.
///
/// Streamed tool calls are the awkward part: the name arrives once, the
/// arguments arrive as string pieces to be concatenated, and both are keyed by
/// an index rather than by the call id — which itself may only appear in the
/// first fragment.
#[derive(Default)]
struct Assembled {
    content: String,
    reasoning: String,
    calls: Vec<PartialCall>,
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

impl Assembled {
    fn absorb(&mut self, event: &Value, on_delta: Sink<'_>) {
        let choice = &event["choices"][0];
        // Non-streaming replies arrive under `message`; some services also send
        // one final non-delta event. Accepting both costs a line and means a
        // provider that ignores `stream: true` still works.
        let delta = if choice["delta"].is_object() { &choice["delta"] } else { &choice["message"] };

        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            self.content.push_str(text);
            on_delta(Delta::Text(text));
        }
        // Providers disagree about where reasoning lives, and none of them
        // agree with the base spec.
        let thinking = delta["reasoning_content"].as_str().or_else(|| delta["reasoning"].as_str());
        if let Some(text) = thinking.filter(|t| !t.is_empty()) {
            self.reasoning.push_str(text);
            on_delta(Delta::Reasoning(text));
        }

        for call in delta["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let index = call["index"].as_u64().unwrap_or(0) as usize;
            if self.calls.len() <= index {
                self.calls.resize_with(index + 1, PartialCall::default);
            }
            let slot = &mut self.calls[index];

            if let Some(id) = call["id"].as_str() {
                slot.id.push_str(id);
            }
            if let Some(name) = call["function"]["name"].as_str() {
                slot.name.push_str(name);
            }
            if let Some(args) = call["function"]["arguments"].as_str() {
                slot.arguments.push_str(args);
            }
        }
    }

    fn finish(self) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            content: self.content,
            reasoning: (!self.reasoning.trim().is_empty()).then_some(self.reasoning),
            tool_call_id: None,
            tool_calls: self
                .calls
                .into_iter()
                .filter(|c| !c.name.is_empty())
                .map(|c| ToolCall {
                    id: c.id,
                    name: c.name,
                    // A model will occasionally produce arguments that do not
                    // parse. An empty object lets the tool report what is
                    // missing, which the model can act on; an error here would
                    // throw away the turn.
                    arguments: serde_json::from_str(&c.arguments).unwrap_or_else(|_| json!({})),
                })
                .collect(),
        }
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

#[cfg(test)]
mod tests;
