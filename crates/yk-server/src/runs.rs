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
use yk_ai::{ChatMessage, ToolCall};

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
    /// The answer as it is arriving, before the model has finished the message.
    /// Cleared when the message lands as a step, so the same words are never on
    /// screen twice.
    pub partial: String,
    /// The current reasoning, same rule.
    pub partial_reasoning: String,
}

/// Every conversation with a turn in flight, and the last state of those that
/// have finished recently.
#[derive(Default)]
pub struct Runs {
    inner: Mutex<HashMap<String, Arc<Run>>>,
}

/// How often a turn in progress is broadcast.
///
/// Fast enough to read as live, slow enough that a model producing three
/// hundred tokens a second does not turn the event bus into the bottleneck.
const ANNOUNCE_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

pub struct Run {
    pub state: Mutex<RunState>,
    pub cancel: Cancel,
    last_announce: Mutex<std::time::Instant>,
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
            last_announce: Mutex::new(std::time::Instant::now()),
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
        {
            let mut state = self.state.lock();
            state.steps.push(step);
            // Whatever was arriving has now arrived as a step.
            state.partial.clear();
            state.partial_reasoning.clear();
        }
        self.announce();
    }

    /// Append an arriving fragment.
    ///
    /// Announced at most every [`ANNOUNCE_EVERY`], because a token is a poor
    /// unit of anything: a fast model produces hundreds a second, and pushing
    /// the whole state that often would spend more time serialising than the
    /// model spends thinking. The last one always goes out, so the text never
    /// stops short of what arrived.
    fn delta(&self, kind: &str, text: &str) {
        let ready = {
            let mut state = self.state.lock();
            if kind == "reasoning" {
                state.partial_reasoning.push_str(text);
            } else {
                state.partial.push_str(text);
            }
            let mut last = self.last_announce.lock();
            let ready = last.elapsed() >= ANNOUNCE_EVERY;
            if ready {
                *last = std::time::Instant::now();
            }
            ready
        };
        if ready {
            self.announce();
        }
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

        // A message with no tool calls *is* the answer, and the answer is
        // stored beside the trace rather than inside it. Recording it here too
        // printed every reply twice — once as a step on the way, once as the
        // thing arrived at. Only remarks made on the way to an answer belong
        // in the trace.
        if !message.tool_calls.is_empty() && !message.content.trim().is_empty() {
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

    fn delta(&self, kind: &str, text: &str) {
        self.run.delta(kind, text);
    }

    fn cancelled(&self) -> bool {
        self.run.cancel.stopped()
    }
}

/// The steps as they should be persisted with the finished answer.
pub fn steps_json(state: &RunState) -> Value {
    json!(state.steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::event::EventBus;

    fn progress() -> RunProgress {
        let events = EventBus::new(16);
        let runs = Runs::default();
        let run = runs.start(&events, 1, "K1", "why?").unwrap();
        RunProgress { run, writers: vec!["trash_items"] }
    }

    fn assistant(content: &str, calls: Vec<ToolCall>) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            content: content.into(),
            tool_calls: calls,
            ..Default::default()
        }
    }

    fn call() -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: "search_library".into(),
            arguments: serde_json::json!({}),
        }
    }

    #[test]
    fn the_answer_is_not_also_a_step() {
        let p = progress();
        p.said(&assistant("Hello! How can I help with your library?", Vec::new()));

        // A message with no tool calls *is* the answer, and the answer is kept
        // beside the trace. Recording it here too printed every reply twice —
        // once on the way, once as the thing arrived at. A user reported it as
        // "it shows twice", which is exactly what it was.
        assert!(p.run.snapshot().steps.is_empty());
    }

    #[test]
    fn a_remark_on_the_way_to_an_answer_is_a_step() {
        let p = progress();
        p.said(&assistant("Let me look that up.", vec![call()]));

        let steps = p.run.snapshot().steps;
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], Step::Text { content } if content == "Let me look that up."));
    }

    #[test]
    fn reasoning_is_kept_even_for_the_final_message() {
        let p = progress();
        let mut message = assistant("Three papers.", Vec::new());
        message.reasoning = Some("They asked about attention.".into());
        p.said(&message);

        // Working is not the answer, so it belongs in the trace even when the
        // answer beside it does not.
        let steps = p.run.snapshot().steps;
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], Step::Thinking { .. }));
    }

    #[test]
    fn a_finished_step_clears_what_was_arriving() {
        let p = progress();
        p.delta("content", "Let me ");
        p.delta("content", "look.");
        assert_eq!(p.run.snapshot().partial, "Let me look.");

        p.said(&assistant("Let me look.", vec![call()]));

        // The same words must not be on screen twice: once as what is arriving
        // and once as the step it arrived as.
        assert!(p.run.snapshot().partial.is_empty());
    }
}
