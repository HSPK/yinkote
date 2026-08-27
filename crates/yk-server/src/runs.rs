//! A running agent turn.
//!
//! A turn belongs to the **conversation**, not to the HTTP request that started
//! it. Half a minute of searching should not be lost because somebody switched
//! tabs, and it certainly should not be lost because they pressed reload — the
//! model is going to finish either way, so the only question is whether anyone
//! is still there to receive the answer.
//!
//! So `ask` starts a run and returns immediately. Progress arrives on the event
//! bus that every other change already uses, and the current state is readable
//! at any time, which is what lets a fresh page rejoin a turn in flight.
//!
//! Cancellation is a flag the loop checks between steps rather than a dropped
//! future: a request already in flight will arrive regardless, and discarding
//! its answer would only mean paying for it again later.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use yk_agent::{Cancel, Progress};
use yk_core::event::{DomainEvent, EventBus};
use yk_core::ports::{ChatMessage, ToolCall};

/// One entry in a turn, in the order it happened.
///
/// The same shape the client draws and the same shape that is persisted with
/// the finished message, so a turn watched live and a turn read back tomorrow
/// look identical. Two representations of one thing would drift.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Step {
    /// Prose the model produced on its way to the answer.
    Text { content: String },
    /// The model's own reasoning, when it exposes it separately.
    Thinking { content: String },
    Tool {
        name: String,
        arguments: Value,
        result: String,
        writes: bool,
    },
}

/// What a conversation's turn is doing.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub running: bool,
    /// The question that started it, so a rejoining client can show it.
    pub question: String,
    pub steps: Vec<Step>,
    /// Set when the turn ends; empty while it is going.
    pub reply: String,
    pub truncated: bool,
    pub stopped: bool,
    pub error: Option<String>,
}

/// Every conversation with a turn in flight, and the last state of those that
/// have finished recently.
#[derive(Default)]
pub struct Runs {
    inner: Mutex<HashMap<String, Arc<Run>>>,
}

pub struct Run {
    pub state: Mutex<RunState>,
    pub cancel: Cancel,
    events: EventBus,
    library_id: i64,
    conversation: String,
}

impl Runs {
    /// Start a run, or refuse because one is already going.
    ///
    /// Refusing rather than queueing: two turns in one conversation would
    /// interleave their tool calls and produce a transcript nobody can read.
    pub fn start(
        &self,
        events: &EventBus,
        library_id: i64,
        conversation: &str,
        question: &str,
    ) -> Option<Arc<Run>> {
        let mut runs = self.inner.lock();
        if runs.get(conversation).is_some_and(|r| r.state.lock().running) {
            return None;
        }

        let run = Arc::new(Run {
            state: Mutex::new(RunState {
                running: true,
                question: question.to_string(),
                ..Default::default()
            }),
            cancel: Cancel::default(),
            events: events.clone(),
            library_id,
            conversation: conversation.to_string(),
        });
        runs.insert(conversation.to_string(), run.clone());
        run.announce();
        Some(run)
    }

    pub fn get(&self, conversation: &str) -> Option<Arc<Run>> {
        self.inner.lock().get(conversation).cloned()
    }

    /// Drop finished runs, keeping the map from growing for the life of the
    /// process. Called when a run ends, so at most one dead entry per
    /// conversation is ever held.
    pub fn forget_finished(&self, keep: &str) {
        self.inner.lock().retain(|key, run| key == keep || run.state.lock().running);
    }
}

impl Run {
    pub fn snapshot(&self) -> RunState {
        self.state.lock().clone()
    }

    /// Tell everyone watching what the turn looks like now.
    ///
    /// The whole state rather than a delta. It is small — a few steps and a
    /// line of text — and a client that missed one delta while switching tabs
    /// would otherwise be permanently out of step with no way to notice.
    fn announce(&self) {
        self.events.publish(DomainEvent::AgentProgress {
            library_id: self.library_id,
            conversation: self.conversation.clone(),
            state: serde_json::to_value(self.snapshot()).unwrap_or_default(),
        });
    }

    pub fn finish(&self, reply: String, truncated: bool, stopped: bool) {
        {
            let mut state = self.state.lock();
            state.running = false;
            state.reply = reply;
            state.truncated = truncated;
            state.stopped = stopped;
        }
        self.announce();
    }

    pub fn fail(&self, message: String) {
        {
            let mut state = self.state.lock();
            state.running = false;
            state.error = Some(message);
        }
        self.announce();
    }

    fn push(&self, step: Step) {
        self.state.lock().steps.push(step);
        self.announce();
    }
}

/// The bridge from the agent loop to the run.
pub struct RunProgress {
    pub run: Arc<Run>,
    /// Names of the tools that change the library, so a step can be marked
    /// without the loop knowing what any of them do.
    pub writers: Vec<&'static str>,
}

impl Progress for RunProgress {
    fn said(&self, message: &ChatMessage) {
        // Reasoning first: it is what the model was doing *before* it spoke,
        // and showing it afterwards would put the transcript out of order.
        if let Some(thinking) = message.reasoning.as_deref() {
            if !thinking.trim().is_empty() {
                self.run.push(Step::Thinking { content: thinking.to_string() });
            }
        }
        if !message.content.trim().is_empty() {
            self.run.push(Step::Text { content: message.content.clone() });
        }
    }

    fn tool_done(&self, call: &ToolCall, result: &str) {
        self.run.push(Step::Tool {
            name: call.name.clone(),
            arguments: call.arguments.clone(),
            result: result.to_string(),
            writes: self.writers.contains(&call.name.as_str()),
        });
    }

    fn cancelled(&self) -> bool {
        self.run.cancel.stopped()
    }
}

/// The steps as they should be persisted with the finished answer.
pub fn steps_json(state: &RunState) -> Value {
    json!(state.steps)
}
