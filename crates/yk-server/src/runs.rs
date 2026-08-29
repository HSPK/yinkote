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
    /// When the turn started, in epoch milliseconds.
    ///
    /// Sent rather than an elapsed count, so a client that rejoins after a
    /// reload shows the real age of the turn instead of restarting the clock —
    /// and so the number keeps moving between announcements, which arrive only
    /// every hundred milliseconds and stop entirely while the model thinks.
    #[serde(rename = "startedAt")]
    pub started_at: i64,
    /// The question that started it, so a rejoining client can show it.
    pub question: String,
    pub steps: Vec<Step>,
    /// Set when the turn ends; empty while it is going.
    pub reply: String,
    pub truncated: bool,
    pub stopped: bool,
    /// The server's own words, kept for anybody diagnosing.
    pub error: Option<String>,
    /// What *kind* of failure, as a code the catalogues can name.
    ///
    /// The message is written in English at the point it is thrown and often
    /// carries the upstream service's raw JSON. Showing that in a chat bubble
    /// to a reader of any language is the thing this program says it never
    /// does -- and a throttled model is by far the most common failure here,
    /// so it is the one a reader meets.
    #[serde(rename = "errorProblem", skip_serializing_if = "Option::is_none")]
    pub error_problem: Option<&'static str>,
    /// The answer as it is arriving, before the model has finished the message.
    /// Cleared when the message lands as a step, so the same words are never on
    /// screen twice.
    pub partial: String,
    /// The current reasoning, same rule.
    pub partial_reasoning: String,
}

/// The kind of failure, from what the message says.
///
/// By the words rather than by a type, because the message arrives from three
/// layers -- the provider, the agent loop and the store -- and giving each its
/// own error enum to thread through would be a large change for a small
/// question. The words checked are the ones the upstream services actually
/// use; anything unrecognised keeps its sentence, which is better than a
/// wrong label.
fn classify(message: &str) -> &'static str {
    let said = message.to_lowercase();
    if said.contains("429") || said.contains("rate limit") || said.contains("too many requests") {
        "rateLimited"
    } else if said.contains("no model is configured") || said.contains("agent.endpoint") {
        "notConfigured"
    } else if said.contains("timed out") || said.contains("timeout") {
        "timedOut"
    } else if said.contains("connect") || said.contains("dns") || said.contains("unreachable") {
        "unreachable"
    } else if said.contains("401") || said.contains("403") || said.contains("unauthorized") {
        "refused"
    } else {
        "failed"
    }
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
                started_at: yk_core::now_ms(),
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
    /// The whole state rather than a delta, so a client that missed one while
    /// switching tabs is not permanently out of step with no way to notice.
    ///
    /// This used to say the state was small. It was not: one `library_overview`
    /// result made a snapshot 23KB, and a snapshot goes out after *every* step,
    /// so a five-step turn broadcast that five times over — to draw header
    /// lines that show a tool's name and its arguments. The results carry the
    /// same cap as the persisted trace, which is what makes the sentence above
    /// true rather than hopeful.
    fn announce(&self) {
        self.events.publish(DomainEvent::AgentProgress {
            library_id: self.library_id,
            conversation: self.conversation.clone(),
            state: state_json(&self.snapshot()),
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
            state.error_problem = Some(classify(&message));
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

/// How much of a tool's answer is kept in the persisted trace.
///
/// A step is drawn collapsed: a header line with the tool's name and the
/// arguments it was called with. The result is rendered only if somebody
/// expands it, and most never do. Meanwhile one ordinary turn — a 204-byte
/// answer to "how many papers about attention?" — carried 48KB of tool output,
/// 19.7KB of it a single `library_overview` blob, and every later load of that
/// conversation carried it again, for ever.
///
/// The model has already read the full result; this is the human's record of
/// what happened. Enough of it to recognise the answer is what that needs.
const TRACE_RESULT_CHARS: usize = 2000;

/// A whole run state as any client should receive it.
///
/// One function for all three ways it leaves the server — the progress
/// broadcast, the `/run` poll and the persisted trace — because they were
/// three serialisations of the same value and only one of them was capped.
pub fn state_json(state: &RunState) -> Value {
    let mut value = json!(state);
    if let Some(object) = value.as_object_mut() {
        object.insert("steps".into(), steps_json(state));
    }
    value
}

/// The steps as they should be persisted with the finished answer.
///
/// Capped rather than complete. The cut is marked, so a reader is never left
/// believing a tool returned less than it did.
pub fn steps_json(state: &RunState) -> Value {
    let steps: Vec<Value> = state
        .steps
        .iter()
        .map(|step| match step {
            Step::Tool { name, arguments, result, writes } if result.chars().count() > TRACE_RESULT_CHARS => {
                json!({
                    "kind": "tool",
                    "name": name,
                    "arguments": arguments,
                    "result": crate::agent::reading::truncate(result, TRACE_RESULT_CHARS),
                    "writes": writes,
                    "clipped": true,
                })
            }
            other => json!(other),
        })
        .collect();
    json!(steps)
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

    /// A turn's record must not grow without bound.
    ///
    /// One ordinary exchange — a 204-byte answer — persisted 48KB of tool
    /// output, and a conversation is fetched whole every time it is opened.
    /// The step is drawn collapsed, so none of that is even on screen unless
    /// somebody expands it.
    #[test]
    fn a_long_tool_result_is_kept_in_part_and_says_so() {
        let mut state = RunState::default();
        state.steps.push(Step::Tool {
            name: "library_overview".into(),
            arguments: json!({}),
            result: "x".repeat(20_000),
            writes: false,
        });

        let steps = steps_json(&state);
        let step = &steps[0];
        assert_eq!(step["clipped"], json!(true), "a cut answer must say it was cut");
        let kept = step["result"].as_str().unwrap().chars().count();
        assert!(kept <= TRACE_RESULT_CHARS + 1, "kept {kept} characters");
        // The header line is what the step is for, so it survives intact.
        assert_eq!(step["name"], "library_overview");
        assert_eq!(step["writes"], json!(false));
    }

    /// And a short one is untouched — including the absence of the marker,
    /// since a note on every step would tell the reader nothing.
    #[test]
    fn a_short_tool_result_is_left_alone() {
        let mut state = RunState::default();
        state.steps.push(Step::Tool {
            name: "search_library".into(),
            arguments: json!({ "query": "attention" }),
            result: "three hits".into(),
            writes: false,
        });

        let step = &steps_json(&state)[0];
        assert_eq!(step["result"], "three hits");
        assert!(step.get("clipped").is_none(), "an intact result was marked as cut");
    }

    /// Prose and reasoning are the model's own words and are short; cutting
    /// them would lose the part a reader actually reads.
    #[test]
    fn only_tool_results_are_cut() {
        let mut state = RunState::default();
        state.steps.push(Step::Text { content: "y".repeat(9_000) });
        let step = &steps_json(&state)[0];
        assert_eq!(step["content"].as_str().unwrap().chars().count(), 9_000);
    }
}

#[cfg(test)]
mod failure_kinds {
    use super::classify;

    /// A throttled model is the failure a reader of this feature actually
    /// meets, and its message carries the upstream service's raw JSON.
    #[test]
    fn a_throttled_model_is_named() {
        for said in [
            "internal error: model returned 429 Too Many Requests: {\"error\":\"TRAPI: Rate Limit Exceeded\"}",
            "upstream busy: rate limit exceeded",
            "HTTP 429",
        ] {
            assert_eq!(classify(said), "rateLimited", "{said}");
        }
    }

    #[test]
    fn the_other_kinds_are_told_apart() {
        assert_eq!(classify("no model is configured; set agent.endpoint"), "notConfigured");
        assert_eq!(classify("the request timed out after 60s"), "timedOut");
        assert_eq!(classify("error sending request: failed to connect"), "unreachable");
        assert_eq!(classify("model returned 401 Unauthorized"), "refused");
    }

    /// Anything unrecognised keeps its own sentence rather than being given a
    /// wrong label -- a mislabelled failure sends the reader the wrong way.
    #[test]
    fn an_unfamiliar_failure_is_not_guessed_at() {
        assert_eq!(classify("the index refused to answer"), "failed");
        assert_eq!(classify(""), "failed");
    }
}
