//! Summarising an item.
//!
//! Separate from the chat endpoint because the shapes differ: a conversation is
//! open-ended and belongs to the user, whereas a summary is a derived artefact
//! that belongs to the item and should be findable long after whoever asked for
//! it has forgotten. So it is stored as a note child, which means it is
//! searchable, exportable and syncable with no new machinery at all.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_core::event::DomainEvent;
use yk_core::model::{Item, ItemDraft};
use yk_core::ports::ChatMessage;
use yk_core::Error;

use super::{announce, key};
use crate::error::ApiResult;
use crate::state::App;

/// Marks a note as machine-written, so it is never mistaken for the user's own.
pub const SUMMARY_TAG: &str = "summary";

pub fn router() -> Router<App> {
    Router::new().route("/libraries/:lib/items/:key/summarise", post(summarise))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SummariseBody {
    /// What to emphasise, e.g. "focus on the method". Optional.
    focus: Option<String>,
}

async fn summarise(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<SummariseBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let parent = key(&k)?;
    let item = app.store().items.get(lib, &parent).await?;

    let agent = app.agent().ok_or_else(|| {
        Error::invalid("no model is configured; set agent.endpoint and agent.model")
    })?;

    let turn = agent
        .run(lib, vec![ChatMessage::new("user", prompt(&item, body.focus.as_deref()))])
        .await?;

    if turn.reply.trim().is_empty() {
        return Err(Error::internal("the model returned nothing").into());
    }

    // One summary per item: regenerating replaces rather than accumulating a
    // pile of near-identical notes nobody will ever reconcile.
    let existing = app
        .store()
        .items
        .children(lib, &parent)
        .await?
        .into_iter()
        .find(|c| c.item_type == "note" && c.tags.iter().any(|t| t.tag == SUMMARY_TAG));

    let note = match existing {
        Some(note) => {
            app.store()
                .items
                .update(
                    lib,
                    &note.key,
                    serde_json::from_value(json!({ "note": turn.reply })).map_err(internal)?,
                    // No version check: regenerating deliberately overwrites
                    // whatever summary was there.
                    None,
                )
                .await?
        }
        None => {
            let mut draft = ItemDraft::new("note").with_field("note", turn.reply.as_str());
            draft.parent_key = Some(parent.clone());
            draft.tags = vec![yk_core::model::ItemTag {
                tag: SUMMARY_TAG.into(),
                // Type 1 is an automatic tag: the user did not write it.
                r#type: 1,
            }];
            app.store().items.create(lib, draft).await?
        }
    };

    announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![parent.clone(), note.key.clone()],
        version,
    })
    .await?;

    Ok(Json(json!({ "note": note, "model": agent.model(), "truncated": turn.truncated })))
}

/// What the model is asked.
///
/// The metadata is handed over directly rather than left for the agent to look
/// up: it already has the item, and a tool round-trip to fetch what the caller
/// is holding is latency for nothing. Searching the library is still available
/// for context the item itself does not carry.
fn prompt(item: &Item, focus: Option<&str>) -> String {
    let mut out = String::from(
        "Summarise this item for a researcher's own notes. Three or four sentences: what it \
         does, how, and why it matters. Do not repeat the title. Do not invent findings that \
         are not in the material given. If the material is too thin to summarise honestly, \
         say so in one sentence instead of padding.\n\n",
    );

    let creators: Vec<String> = item.creators.iter().map(|c| c.display()).collect();
    for (label, value) in [
        ("Title", item.title()),
        ("Type", item.item_type.as_str()),
        ("Date", item.field("date").unwrap_or_default()),
        ("Publication", item.field("publicationTitle").unwrap_or_default()),
        ("DOI", item.field("DOI").unwrap_or_default()),
        ("Abstract", item.field("abstractNote").unwrap_or_default()),
    ] {
        if !value.is_empty() {
            out.push_str(&format!("{label}: {value}\n"));
        }
    }
    if !creators.is_empty() {
        out.push_str(&format!("Authors: {}\n", creators.join(", ")));
    }
    if let Some(focus) = focus.map(str::trim).filter(|f| !f.is_empty()) {
        out.push_str(&format!("\nThe reader asked you to focus on: {focus}\n"));
    }
    out
}

fn internal(e: serde_json::Error) -> Error {
    Error::internal(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::{Creator, Fields};
    use yk_core::Key;

    fn item() -> Item {
        let mut fields = Fields::new();
        fields.insert("title".into(), json!("Attention Is All You Need"));
        fields.insert("abstractNote".into(), json!("We propose the Transformer."));
        Item {
            key: Key::generate(),
            library_id: 1,
            item_type: "journalArticle".into(),
            parent_key: None,
            fields,
            creators: vec![Creator {
                last_name: Some("Vaswani".into()),
                first_name: Some("Ashish".into()),
                ..Default::default()
            }],
            tags: Vec::new(),
            collections: Vec::new(),
            version: 1,
            deleted: false,
            date_added: 0,
            date_modified: 0,
        }
    }

    #[test]
    fn the_prompt_carries_the_metadata_rather_than_making_the_agent_fetch_it() {
        let text = prompt(&item(), None);
        assert!(text.contains("Attention Is All You Need"));
        assert!(text.contains("We propose the Transformer."));
        assert!(text.contains("Vaswani"));
    }

    #[test]
    fn empty_fields_are_left_out_rather_than_sent_blank() {
        // "DOI: " teaches a model that blanks are normal and invites it to fill
        // them in.
        let text = prompt(&item(), None);
        assert!(!text.contains("DOI:"));
        assert!(!text.contains("Publication:"));
    }

    #[test]
    fn a_focus_is_passed_through_but_a_blank_one_is_not() {
        assert!(prompt(&item(), Some("the method")).contains("focus on: the method"));
        assert!(!prompt(&item(), Some("   ")).contains("focus on"));
        assert!(!prompt(&item(), None).contains("focus on"));
    }

    #[test]
    fn the_model_is_told_not_to_pad_a_thin_record() {
        assert!(prompt(&item(), None).contains("too thin"));
    }
}
