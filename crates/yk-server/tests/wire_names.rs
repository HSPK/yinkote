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
    use yk_server::runs::{RunState, Step};
    use yk_server::tasks::Tasks;

    // Harvesting used to have a shape of its own here; it is a task now, and
    // the guard moves with the value that is actually on the wire.
    let tasks = Tasks::default();
    let task = tasks.start("harvest", "Fetching");
    task.progress("Fetching", 1, 2);
    assert_camel("TaskState", task.snapshot());

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

/// The Word pane reads the same shapes, and it is not in this workspace's type
/// system.
///
/// The pane is plain script served out of the binary, so nothing checks that
/// the name it reads is the name the server sends. The first draft read
/// `plan.updated`; the field is serialised as `updatedFields`, and the symptom
/// would have been a pane that inserted citations and then left every one of
/// them empty — with no error anywhere, because reading a missing property is
/// not an error in JavaScript.
#[test]
fn the_word_pane_reads_names_the_server_actually_sends() {
    use yk_server::integration::document::{Entry, Plan, Rendered};

    let plan = serde_json::to_value(Plan {
        updated: vec![Rendered { id: "1".into(), text: "[1]".into() }],
        bibliography: vec![Entry { key: "AAAA".into(), text: "…".into() }],
    })
    .unwrap();
    let sent: Vec<&String> = plan.as_object().unwrap().keys().collect();

    // Comment lines are dropped rather than scanned: the file explains this
    // very mistake in prose, and a guard that cannot tell an explanation from
    // an instruction is a guard people learn to switch off. Only whole comment
    // lines are removed, so a `//` inside a URL literal is left alone.
    let source: String = include_str!("../src/addin/assets/taskpane.js")
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut read = Vec::new();
    for (at, _) in source.match_indices("plan.") {
        let rest = &source[at + 5..];
        let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
        if end > 0 {
            read.push(&rest[..end]);
        }
    }
    assert!(!read.is_empty(), "the scan found nothing; has the pane been rewritten?");

    for name in read {
        assert!(
            sent.iter().any(|k| *k == name),
            "the pane reads `plan.{name}`, but a Plan is sent as {sent:?}. \
             Reading a missing property is not an error in JavaScript, so this \
             would ship as a pane that quietly does nothing."
        );
    }
}
