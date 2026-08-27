//! Tests for the OpenAI dialect.
//!
//! The assembler is the part worth testing without a network: streamed tool
//! calls arrive in pieces keyed by index, the name comes once, the arguments
//! come as string fragments, and getting any of that wrong produces a turn
//! that looks fine and calls the wrong thing.

use super::*;

fn event(value: serde_json::Value) -> serde_json::Value {
    value
}

fn assemble(events: &[serde_json::Value]) -> (ChatMessage, Vec<String>) {
    let seen = std::sync::Mutex::new(Vec::new());
    let mut assembled = Assembled::default();
    let sink = |delta: Delta<'_>| {
        seen.lock().unwrap().push(format!("{}:{}", delta.kind(), delta.text()));
    };
    for e in events {
        assembled.absorb(e, &sink);
    }
    (assembled.finish(), seen.into_inner().unwrap())
}

#[test]
fn joins_the_answer_out_of_fragments() {
    let (message, deltas) = assemble(&[
        event(json!({ "choices": [{ "delta": { "content": "Three " } }] })),
        event(json!({ "choices": [{ "delta": { "content": "papers." } }] })),
    ]);

    assert_eq!(message.content, "Three papers.");
    // Reported as they arrive, or a turn shows nothing until it is over.
    assert_eq!(deltas, vec!["content:Three ", "content:papers."]);
}

#[test]
fn keeps_reasoning_apart_from_the_answer() {
    let (message, deltas) = assemble(&[
        event(json!({ "choices": [{ "delta": { "reasoning_content": "They want " } }] })),
        event(json!({ "choices": [{ "delta": { "reasoning": "papers." } }] })),
        event(json!({ "choices": [{ "delta": { "content": "Here they are." } }] })),
    ]);

    // Working is not an answer. Merging them would present a draft as a
    // conclusion, and both spellings are in the wild.
    assert_eq!(message.reasoning.as_deref(), Some("They want papers."));
    assert_eq!(message.content, "Here they are.");
    assert_eq!(deltas[0], "reasoning:They want ");
}

#[test]
fn rebuilds_a_tool_call_from_its_pieces() {
    let (message, _) = assemble(&[
        event(json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "id": "call_1", "function": { "name": "search_library" } }
        ] } }] })),
        event(json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "function": { "arguments": "{\"query\":" } }
        ] } }] })),
        event(json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "function": { "arguments": "\"attention\"}" } }
        ] } }] })),
    ]);

    assert_eq!(message.tool_calls.len(), 1);
    assert_eq!(message.tool_calls[0].id, "call_1");
    assert_eq!(message.tool_calls[0].name, "search_library");
    assert_eq!(message.tool_calls[0].arguments["query"], "attention");
}

#[test]
fn keeps_two_calls_apart_by_their_index() {
    let (message, _) = assemble(&[
        event(json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "id": "a", "function": { "name": "get_item", "arguments": "{}" } },
            { "index": 1, "id": "b", "function": { "name": "trash_items", "arguments": "{}" } }
        ] } }] })),
    ]);

    // Interleaved fragments keyed by index are the whole difficulty here;
    // merging them would call one tool with another's arguments.
    let names: Vec<&str> = message.tool_calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["get_item", "trash_items"]);
}

#[test]
fn survives_arguments_that_do_not_parse() {
    let (message, _) = assemble(&[event(json!({ "choices": [{ "delta": { "tool_calls": [
        { "index": 0, "id": "a", "function": { "name": "get_item", "arguments": "{not json" } }
    ] } }] }))]);

    // An empty object lets the tool say what is missing, which the model can
    // act on. Failing here would throw away the whole turn.
    assert_eq!(message.tool_calls[0].arguments, json!({}));
}

#[test]
fn accepts_a_provider_that_ignores_the_streaming_flag() {
    // Some services answer a `stream: true` request with one ordinary reply
    // under `message`. Accepting both costs a line and works either way.
    let (message, _) = assemble(&[event(json!({
        "choices": [{ "message": { "role": "assistant", "content": "All at once." } }]
    }))]);

    assert_eq!(message.content, "All at once.");
}

#[test]
fn a_call_with_no_name_is_not_a_call() {
    let (message, _) = assemble(&[event(json!({ "choices": [{ "delta": { "tool_calls": [
        { "index": 0, "function": { "arguments": "{}" } }
    ] } }] }))]);

    assert!(message.tool_calls.is_empty());
}

#[test]
fn round_trips_a_tool_call_through_the_wire_shape() {
    let message = ChatMessage {
        role: "assistant".into(),
        tool_calls: vec![ToolCall {
            id: "call_1".into(),
            name: "search_library".into(),
            arguments: json!({ "query": "attention" }),
        }],
        ..Default::default()
    };

    // Arguments go out as a JSON *string*, which is the part of this dialect
    // most likely to be got wrong by hand.
    let wire = to_wire(&message);
    assert_eq!(wire["tool_calls"][0]["function"]["arguments"], "{\"query\":\"attention\"}");
}
