//! The agent loop.
//!
//! Deliberately provider-agnostic and store-agnostic: it takes a
//! [`ChatProvider`] and a set of [`Tool`]s and runs the conversation between
//! them. Everything that knows about HTTP, SQLite or a particular model lives
//! outside this crate, which is why the loop can be tested against a scripted
//! provider with no network and no database.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;
use yk_ai::stream::Delta;
use yk_ai::{ChatMessage, ChatProvider, ChatRequest, Tool, ToolCall};
use yk_core::{Error, Result};

/// How many times the model may call tools before we stop.
///
/// A model that keeps calling tools is either exploring usefully or stuck in a
/// loop, and from the outside those look identical. Stopping at a bound turns
/// the second case into a slightly short answer instead of an unbounded bill.
pub const MAX_STEPS: usize = 6;

/// What the loop produced.
#[derive(Debug, Clone)]
pub struct AgentTurn {
    /// The assistant's final message.
    pub reply: String,
    /// Every message generated this turn, including tool traffic, in order.
    /// Persisted so a conversation can be re-read and audited later.
    pub transcript: Vec<ChatMessage>,
    /// True when the loop stopped at [`MAX_STEPS`] rather than because the
    /// model was finished. Surfaced rather than hidden: a truncated answer the
    /// reader knows about is far better than one they do not.
    pub truncated: bool,
    /// True when the *user* stopped it, as opposed to the loop running out of
    /// steps. Both are truncated; only one of them is a surprise.
    pub stopped: bool,
}

/// Told about a turn as it happens.
///
/// The loop reports rather than returns, because a turn that takes half a
/// minute and shows nothing until it ends is indistinguishable from one that
/// has hung — and because a reader who can see the searches can judge the
/// answer, while one who is shown them afterwards can only take it on trust.
pub trait Progress: Send + Sync {
    /// The model said something, tool calls or not.
    fn said(&self, _message: &ChatMessage) {}
    /// A tool was called and answered.
    fn tool_done(&self, _call: &ToolCall, _result: &str) {}
    /// Part of the answer arrived. `kind` is `content` or `reasoning`.
    fn delta(&self, _kind: &str, _text: &str) {}
    /// Whether the caller has asked for this to stop.
    fn cancelled(&self) -> bool {
        false
    }
}

/// Why a turn ended early, when it did.
///
/// Reported rather than hidden. An answer the reader knows is partial is far
/// more useful than one they believe is complete — and the two reasons are not
/// the same thing: `stopped` is what they asked for, `budget` is the loop
/// giving up.
fn cut_short(transcript: Vec<ChatMessage>, reason: &str) -> AgentTurn {
    AgentTurn {
        reply: transcript
            .iter()
            .rev()
            .find(|m| m.role == "assistant" && !m.content.is_empty())
            .map(|m| m.content.clone())
            .unwrap_or_default(),
        transcript,
        truncated: true,
        stopped: reason == "stopped",
    }
}

/// The do-nothing observer, for callers that just want an answer.
pub struct Silent;
impl Progress for Silent {}

/// A cancellation flag anybody can hold.
#[derive(Debug, Default)]
pub struct Cancel(AtomicBool);

impl Cancel {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub struct Agent {
    provider: Arc<dyn ChatProvider>,
    tools: HashMap<String, Arc<dyn Tool>>,
    system: String,
}

impl Agent {
    pub fn new(provider: Arc<dyn ChatProvider>, tools: Vec<Arc<dyn Tool>>, system: String) -> Self {
        let tools = tools.into_iter().map(|t| (t.spec().name, t)).collect();
        Self { provider, tools, system }
    }

    /// The tools this agent was built with, sorted so the list is stable.
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn model(&self) -> String {
        self.provider.model()
    }

    /// Run one turn: send the history, satisfy any tool calls, repeat until the
    /// model answers in prose or the step budget runs out.
    pub async fn run(&self, library_id: i64, history: Vec<ChatMessage>) -> Result<AgentTurn> {
        self.run_with(library_id, history, &Silent).await
    }

    /// Run one turn, reporting each step as it happens and stopping when asked.
    ///
    /// Cancellation is checked *between* steps rather than mid-request: a call
    /// already in flight is going to arrive whatever we do, and throwing away
    /// its answer would only mean asking again later.
    pub async fn run_with(
        &self,
        library_id: i64,
        history: Vec<ChatMessage>,
        progress: &dyn Progress,
    ) -> Result<AgentTurn> {
        let specs: Vec<_> = self.tools.values().map(|t| t.spec()).collect();

        let mut messages = Vec::with_capacity(history.len() + 1);
        messages.push(ChatMessage::new("system", &self.system));
        messages.extend(history);

        let mut transcript = Vec::new();

        for step in 0..MAX_STEPS {
            if progress.cancelled() {
                return Ok(cut_short(transcript, "stopped"));
            }

            let reply = self
                .provider
                .complete(
                    ChatRequest { messages: messages.clone(), tools: specs.clone() },
                    &|delta: Delta<'_>| progress.delta(delta.kind(), delta.text()),
                )
                .await?;

            messages.push(reply.clone());
            transcript.push(reply.clone());
            progress.said(&reply);

            if reply.tool_calls.is_empty() {
                return Ok(AgentTurn {
                    reply: reply.content,
                    transcript,
                    truncated: false,
                    stopped: false,
                });
            }

            tracing::debug!(step, calls = reply.tool_calls.len(), "agent tool step");
            for call in &reply.tool_calls {
                if progress.cancelled() {
                    return Ok(cut_short(transcript, "stopped"));
                }
                let result = self.invoke(library_id, call).await;
                progress.tool_done(call, &result.to_string());
                let message = ChatMessage {
                    role: "tool".into(),
                    content: result.to_string(),
                    tool_calls: Vec::new(),
                    tool_call_id: Some(call.id.clone()),
                    reasoning: None,
                };
                messages.push(message.clone());
                transcript.push(message);
            }
        }

        // Out of steps with tool calls still outstanding. Say so plainly rather
        // than returning the last tool result as if it were an answer.
        Ok(cut_short(transcript, "budget"))
    }

    /// Run one tool, turning any failure into a result the model can read.
    ///
    /// A tool that errors must not end the turn: models recover from "that
    /// search found nothing" perfectly well, and an exception here would lose
    /// the work already done.
    async fn invoke(&self, library_id: i64, call: &ToolCall) -> serde_json::Value {
        let Some(tool) = self.tools.get(&call.name) else {
            return json!({ "error": format!("no such tool '{}'", call.name) });
        };
        match tool.call(library_id, call.arguments.clone()).await {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(tool = call.name, %error, "agent tool failed");
                json!({ "error": error.to_string() })
            }
        }
    }
}

/// Convenience for adapters: reject a tool argument that is missing or empty.
pub fn required_str(args: &serde_json::Value, name: &str) -> Result<String> {
    args.get(name)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("tool argument '{name}' is required")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Mutex;
    use yk_ai::stream::Sink;
    use yk_ai::ToolSpec;

    /// A provider that replays a script, so the loop is tested and not the model.
    struct Scripted {
        replies: Mutex<Vec<ChatMessage>>,
        seen: Mutex<Vec<Vec<ChatMessage>>>,
    }

    impl Scripted {
        fn new(replies: Vec<ChatMessage>) -> Arc<Self> {
            Arc::new(Self { replies: Mutex::new(replies), seen: Mutex::new(Vec::new()) })
        }
    }

    #[async_trait]
    impl ChatProvider for Scripted {
        fn model(&self) -> String {
            "scripted".into()
        }
        async fn complete(&self, request: ChatRequest, _on: Sink<'_>) -> Result<ChatMessage> {
            self.seen.lock().unwrap().push(request.messages);
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                return Ok(ChatMessage::new("assistant", "done"));
            }
            Ok(replies.remove(0))
        }
    }

    struct Echo {
        name: &'static str,
        fail: bool,
    }

    #[async_trait]
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name.into(),
                description: "echoes".into(),
                parameters: json!({ "type": "object" }),
            }
        }
        async fn call(&self, library_id: i64, arguments: Value) -> Result<Value> {
            if self.fail {
                return Err(Error::internal("tool exploded"));
            }
            Ok(json!({ "library": library_id, "args": arguments }))
        }
    }

    fn call(id: &str, name: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: vec![ToolCall { id: id.into(), name: name.into(), arguments: json!({}) }],
            reasoning: None,
            tool_call_id: None,
        }
    }

    fn agent(provider: Arc<Scripted>, tools: Vec<Arc<dyn Tool>>) -> Agent {
        Agent::new(provider, tools, "be brief".into())
    }

    #[tokio::test]
    async fn answers_without_tools_when_none_are_needed() {
        let provider = Scripted::new(vec![ChatMessage::new("assistant", "42")]);
        let turn = agent(provider.clone(), vec![]).run(1, vec![ChatMessage::new("user", "hi")])
            .await
            .unwrap();

        assert_eq!(turn.reply, "42");
        assert!(!turn.truncated);
        assert_eq!(turn.transcript.len(), 1);
    }

    #[tokio::test]
    async fn prepends_the_system_prompt_exactly_once() {
        let provider = Scripted::new(vec![call("1", "echo"), ChatMessage::new("assistant", "ok")]);
        agent(provider.clone(), vec![Arc::new(Echo { name: "echo", fail: false })])
            .run(1, vec![ChatMessage::new("user", "hi")])
            .await
            .unwrap();

        for sent in provider.seen.lock().unwrap().iter() {
            assert_eq!(sent.iter().filter(|m| m.role == "system").count(), 1);
            assert_eq!(sent[0].role, "system");
        }
    }

    #[tokio::test]
    async fn feeds_tool_results_back_before_asking_again() {
        let provider = Scripted::new(vec![call("1", "echo"), ChatMessage::new("assistant", "ok")]);
        let turn = agent(provider.clone(), vec![Arc::new(Echo { name: "echo", fail: false })])
            .run(7, vec![ChatMessage::new("user", "hi")])
            .await
            .unwrap();

        assert_eq!(turn.reply, "ok");
        let second = &provider.seen.lock().unwrap()[1];
        let tool = second.iter().find(|m| m.role == "tool").expect("tool result was sent back");
        assert_eq!(tool.tool_call_id.as_deref(), Some("1"));
        assert!(tool.content.contains("\"library\":7"), "the tool sees the right library");
    }

    #[tokio::test]
    async fn a_failing_tool_becomes_a_result_the_model_can_read() {
        let provider = Scripted::new(vec![call("1", "boom"), ChatMessage::new("assistant", "sorry")]);
        let turn = agent(provider.clone(), vec![Arc::new(Echo { name: "boom", fail: true })])
            .run(1, vec![ChatMessage::new("user", "hi")])
            .await
            .unwrap();

        assert_eq!(turn.reply, "sorry", "the turn survives a broken tool");
        assert!(turn.transcript.iter().any(|m| m.role == "tool" && m.content.contains("error")));
    }

    #[tokio::test]
    async fn an_unknown_tool_is_reported_rather_than_fatal() {
        let provider = Scripted::new(vec![call("1", "ghost"), ChatMessage::new("assistant", "ok")]);
        let turn = agent(provider, vec![]).run(1, vec![ChatMessage::new("user", "hi")])
            .await
            .unwrap();

        assert_eq!(turn.reply, "ok");
        assert!(turn.transcript.iter().any(|m| m.content.contains("no such tool")));
    }

    #[tokio::test]
    async fn stops_at_the_step_budget_and_admits_it() {
        // A model stuck calling tools forever looks exactly like one exploring
        // usefully, so the only safe move is a bound.
        let provider = Scripted::new((0..MAX_STEPS + 4).map(|i| call(&i.to_string(), "echo")).collect());
        let turn = agent(provider, vec![Arc::new(Echo { name: "echo", fail: false })])
            .run(1, vec![ChatMessage::new("user", "hi")])
            .await
            .unwrap();

        assert!(turn.truncated, "a truncated answer must say so");
        assert_eq!(turn.transcript.iter().filter(|m| m.role == "assistant").count(), MAX_STEPS);
    }

    #[test]
    fn required_str_rejects_blank_arguments() {
        assert!(required_str(&json!({ "q": "diffusion" }), "q").is_ok());
        assert!(required_str(&json!({ "q": "   " }), "q").is_err());
        assert!(required_str(&json!({}), "q").is_err());
    }
}
