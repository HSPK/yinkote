//! Every shape that crosses the API boundary is camelCase.
//!
//! Written after a field that was not cost a whole feature its point: the gaps
//! view sent `cited_by`, the client read `citedBy`, and the count column — the
//! entire reason that view exists — was blank from the day it shipped. Nothing
//! failed, because the client's test fixture had been copied from the client's
//! own type and so agreed with it about a contract neither had checked.
//!
//! A convention nobody can enforce is a convention that will be broken again,
//! so this enforces it: serialise one of each and look at the keys.

use serde::Serialize;
use serde_json::Value;

/// Every key in the shape, however deeply nested.
fn keys(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                into.push(k.clone());
                keys(v, into);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| keys(v, into)),
        _ => {}
    }
}

#[track_caller]
fn assert_camel<T: Serialize>(what: &str, value: T) {
    let json = serde_json::to_value(value).expect("serialises");
    let mut found = Vec::new();
    keys(&json, &mut found);

    let snake: Vec<&String> = found.iter().filter(|k| k.contains('_')).collect();
    assert!(
        snake.is_empty(),
        "{what} would reach the client with snake_case keys: {snake:?}. \
         Every other response is camelCase, so a client written against the rest \
         of the API silently reads `undefined` from these."
    );
}

#[test]
fn library_shapes_speak_camel_case() {
    use yk_store::relations::Missing;
    use yk_store::{Citation, Download};

    assert_camel(
        "Missing",
        Missing {
            fingerprint: "doi:10 1 x".into(),
            label: "A work".into(),
            year: Some(2017),
            doi: "10.1/x".into(),
            cited_by: 7,
        },
    );

    assert_camel(
        "Citation",
        Citation {
            position: 0,
            key: None,
            label: "A work".into(),
            year: Some(2017),
            fingerprint: "doi:10 1 x".into(),
            doi: "10.1/x".into(),
        },
    );

    assert_camel(
        "Download",
        Download {
            id: 1,
            item_key: "AAAA1111".into(),
            url: "https://example.org/a.pdf".into(),
            state: "waiting".into(),
            attempts: 0,
            error: String::new(),
            title: "A paper".into(),
            bytes: 0,
            updated_at: 0,
        },
    );
}

#[test]
fn graph_shapes_speak_camel_case() {
    use yk_store::{Neighbour, Relation};

    assert_camel(
        "Neighbour",
        Neighbour {
            key: yk_core::Key::generate(),
            title: "A paper".into(),
            year: Some(2017),
            item_type: "journalArticle".into(),
            relation: Relation::Tag,
            weight: 1.0,
        },
    );
}

#[test]
fn agent_shapes_speak_camel_case() {
    use yk_server::routes::Harvest;
    use yk_server::runs::{RunState, Step};

    assert_camel("Harvest", Harvest::default());

    assert_camel(
        "RunState",
        RunState {
            running: true,
            started_at: 1_700_000_000_000,
            question: "why?".into(),
            steps: vec![
                Step::Text { content: "Looking.".into() },
                Step::Thinking { content: "Hmm.".into() },
                Step::Tool {
                    name: "search_library".into(),
                    arguments: serde_json::json!({ "query": "x" }),
                    result: "{}".into(),
                    writes: false,
                },
            ],
            reply: String::new(),
            truncated: false,
            stopped: false,
            error: None,
            partial: String::new(),
            partial_reasoning: String::new(),
        },
    );
}

#[test]
fn plugin_shapes_speak_camel_case() {
    use yk_core::plugin::HookOutcome;

    assert_camel(
        "HookOutcome",
        HookOutcome {
            plugin_id: "crossref".into(),
            result: serde_json::json!({}),
            duration_ms: 3,
            error: None,
        },
    );
}
