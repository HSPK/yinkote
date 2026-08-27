//! The relationship graph around one item.
//!
//! Always a neighbourhood, never the whole library. A hundred thousand nodes is
//! not a picture — it is a grey disc — and the question a graph actually
//! answers is "what is this next to", which is local.
//!
//! Structural edges come from the store, which can find them with an indexed
//! query. Similarity edges come from the search engine, which is the only thing
//! holding the vectors. They are merged here rather than in either, because
//! neither should have to know about the other to answer its own question.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::Key;

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new().route("/libraries/:lib/graph/:key", get(neighbourhood))
}

#[derive(Deserialize)]
struct Params {
    /// How many neighbours of each kind. Kept modest on purpose: a readable
    /// graph is a small one.
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    8
}

/// The most a caller may ask for.
///
/// Not because the query is slow, but because past a few dozen nodes nobody can
/// read the result and the only honest thing to do is refuse.
const MAX_LIMIT: u32 = 30;

async fn neighbourhood(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Query(params): Query<Params>,
) -> ApiResult<Json<serde_json::Value>> {
    let focus = key(&k)?;
    let limit = params.limit.clamp(1, MAX_LIMIT);

    let item = app.store().items.get(lib, &focus).await?;
    let structural = app.store().graph.neighbours(lib, &focus, limit).await?;
    let similar = app.search().similar(lib, &focus, limit as usize).await?;
    let cites = app.store().relations.cites(lib, &focus).await.unwrap_or_default();
    let cited_by = app.store().relations.cited_by(lib, &focus).await.unwrap_or_default();

    // One node per item, however many reasons connect it. Drawing an item three
    // times because it shares a tag, an author and a shelf would say "three
    // papers" when the truth is "one paper, three times over".
    let mut nodes: HashMap<String, serde_json::Value> = HashMap::new();
    let mut edges = Vec::new();

    for n in &structural {
        nodes.entry(n.key.to_string()).or_insert_with(|| {
            json!({
                "key": n.key,
                "title": n.title,
                "year": n.year,
                "itemType": n.item_type,
            })
        });
        edges.push(json!({
            "source": focus,
            "target": n.key,
            "relation": n.relation,
            "weight": n.weight,
        }));
    }

    let known: Vec<Key> = similar.iter().map(|(key, _)| key.clone()).collect();
    let titles = titles_for(&app, lib, &known).await;

    for (key, score) in similar {
        if key == focus {
            continue;
        }
        let Some(found) = titles.get(key.as_str()) else { continue };
        nodes.entry(key.to_string()).or_insert_with(|| found.clone());
        edges.push(json!({
            "source": focus,
            "target": key,
            "relation": "similar",
            // The cosine, kept as it is: a similarity edge that claims the same
            // strength as a shared tag would be lying about where it came from.
            "weight": score,
        }));
    }

    // Citations are the only edges here that can point outside the library, and
    // that is most of their value: a work cited by several papers on the shelf
    // and owned by none is, almost by definition, the next thing to read.
    for (citation, outgoing) in cites
        .iter()
        .map(|c| (c, true))
        .chain(cited_by.iter().map(|c| (c, false)))
        .take(limit as usize * 2)
    {
        let id = match &citation.key {
            Some(key) => key.to_string(),
            // Something not owned still deserves a stable identity in the
            // picture, or it would be drawn twice when two papers cite it.
            None if !citation.fingerprint.is_empty() => citation.fingerprint.clone(),
            None => format!("ref:{}:{}", focus, citation.position),
        };
        if id == focus.to_string() {
            continue;
        }

        nodes.entry(id.clone()).or_insert_with(|| {
            json!({
                "key": id,
                "title": citation.label,
                "year": citation.year,
                "itemType": "journalArticle",
                // Drawn differently, because clicking it cannot open anything.
                "external": citation.key.is_none(),
            })
        });

        let (source, target) =
            if outgoing { (focus.to_string(), id) } else { (id, focus.to_string()) };
        edges.push(json!({
            "source": source,
            "target": target,
            "relation": "cites",
            "weight": 1.0,
        }));
    }

    let focus_node = json!({
        "key": item.key,
        "title": item.title(),
        "year": item.field("date").and_then(year),
        "itemType": item.item_type,
        "focus": true,
    });

    let mut all: Vec<serde_json::Value> = vec![focus_node];
    all.extend(nodes.into_values());

    Ok(Json(json!({ "focus": focus, "nodes": all, "edges": edges })))
}

/// Look up the labels for keys the search engine returned.
///
/// A node with no label is a dot, and a dot is not information.
async fn titles_for(app: &App, lib: i64, keys: &[Key]) -> HashMap<String, serde_json::Value> {
    // One query, not one per node. A neighbourhood is dozens of keys, and a
    // query each is dozens of round trips holding a pooled connection for a
    // request that should be a single read — the shape that turned an archive
    // import into "database is locked" elsewhere in the program.
    //
    // A key with no item is simply absent: the caller draws what it was given
    // labels for, and a node nobody can name is a dot.
    app.store()
        .items
        .get_many(lib, keys)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            (
                item.key.to_string(),
                json!({
                    "key": item.key,
                    "title": item.title(),
                    "year": item.field("date").and_then(year),
                    "itemType": item.item_type,
                }),
            )
        })
        .collect()
}

fn year(date: &str) -> Option<i64> {
    let chars: Vec<char> = date.chars().collect();
    chars
        .windows(4)
        .find(|w| w.iter().all(char::is_ascii_digit))
        .and_then(|w| w.iter().collect::<String>().parse().ok())
}
