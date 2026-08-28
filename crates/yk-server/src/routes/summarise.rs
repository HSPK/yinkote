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
use yk_ai::ChatMessage;
use yk_core::Error;

use super::{announce, key};
use crate::error::ApiResult;
use crate::state::App;

/// Marks a note as machine-written, so it is never mistaken for the user's own.
pub const SUMMARY_TAG: &str = "summary";

/// Marks a summary the model did not get to finish.
///
/// On the note rather than only in the reply, because a note outlives the
/// request that made it: the toast is gone in three seconds and the note is
/// still there next year, reading like a complete summary that stops mid
/// thought. Being a tag, it is also searchable — "show me the summaries worth
/// regenerating" is a real question.
pub const TRUNCATED_TAG: &str = "summary-incomplete";

/// Type 1 is an automatic tag: the user did not write it.
const AUTOMATIC: u8 = 1;

/// The tags a summary note should carry, given how the run ended.
fn summary_tags(truncated: bool) -> Vec<yk_core::model::ItemTag> {
    let mut tags =
        vec![yk_core::model::ItemTag { tag: SUMMARY_TAG.into(), r#type: AUTOMATIC }];
    if truncated {
        tags.push(yk_core::model::ItemTag { tag: TRUNCATED_TAG.into(), r#type: AUTOMATIC });
    }
    tags
}

/// How a summary note is listed: the opening of the summary itself.
fn summary_title(reply: &str) -> String {
    yk_core::text::note_title(reply, yk_core::text::NOTE_TITLE_CHARS).to_string()
}

/// The patch that makes an existing note hold this summary.
///
/// **`fields` is a nested object on `ItemPatch`, not a flattened one.** This
/// sent `{"note": ...}` at the top level, where serde matched nothing and
/// produced a patch that was `None` throughout — so regenerating a summary
/// answered 200, changed nothing, and reported "Summary added" over the old
/// text. A patch shape that silently means "do nothing" is the worst kind of
/// mistake to make, because every layer above it looks like it worked.
///
/// The title goes in too: it is the only part of a note most screens show, and
/// it was left describing the summary it had just replaced. Tags go in for the
/// same reason in reverse — regenerating a truncated summary successfully has
/// to *clear* the warning, or the first bad run marks the note for good.
fn summary_fields(reply: &str, truncated: bool) -> serde_json::Value {
    json!({
        "fields": { "note": reply, "title": summary_title(reply) },
        "tags": summary_tags(truncated),
    })
}

/// A fresh note holding this summary.
///
/// Built rather than deserialised from `summary_fields`: `ItemDraft` requires
/// `itemType`, so that round trip failed outright — every new summary would
/// have been a 500. The two paths share the *derivations* instead, which is
/// where the drift was, and neither depends on the other's shape.
fn summary_draft(reply: &str, truncated: bool) -> ItemDraft {
    let mut draft = ItemDraft::new("note")
        .with_field("note", reply)
        .with_field("title", summary_title(reply).as_str());
    draft.tags = summary_tags(truncated);
    draft
}

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
                    serde_json::from_value(summary_fields(&turn.reply, turn.truncated))
                        .map_err(internal)?,
                    // No version check: regenerating deliberately overwrites
                    // whatever summary was there.
                    None,
                )
                .await?
        }
        None => {
            let mut draft = summary_draft(&turn.reply, turn.truncated);
            draft.parent_key = Some(parent.clone());
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
            attachments: Vec::new(),
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

    /// Replacing a summary must rewrite the title too.
    ///
    /// It did not: the update path patched `note` alone, so the note went on
    /// being *listed* under a sentence from the summary it had just replaced.
    /// The title is the only part of a note most screens ever show, which is
    /// what made this invisible in the response and obvious in the sidebar.
    /// The patch must actually be a patch.
    ///
    /// `ItemPatch` keeps its fields in a nested `fields` object and ignores
    /// anything else, so `{"note": ...}` deserialised into a patch that was
    /// entirely `None`: regenerating a summary answered 200 and changed
    /// nothing. Nothing above this could tell, which is why the check has to
    /// be here, on the deserialised patch rather than on the JSON.
    #[test]
    fn a_replaced_summary_actually_replaces_it() {
        let patch: yk_core::model::ItemPatch =
            serde_json::from_value(summary_fields("Evaluates on three benchmarks.", false))
                .unwrap();
        let fields = patch.fields.expect("the patch changes no fields at all");
        assert_eq!(
            fields.get("note").and_then(|v| v.as_str()),
            Some("Evaluates on three benchmarks."),
        );
        assert!(
            fields.get("title").and_then(|v| v.as_str()).unwrap().starts_with("Evaluates on"),
            "the title would go on describing the summary it replaced",
        );
        assert!(patch.tags.is_some(), "the truncation warning would never be cleared");
    }

    /// A note outlives the toast that announced it.
    #[test]
    fn an_unfinished_summary_says_so_on_the_note() {
        let tags = summary_fields("Half an ans", true)["tags"].clone();
        let names: Vec<String> =
            tags.as_array().unwrap().iter().map(|t| t["tag"].as_str().unwrap().into()).collect();
        assert!(names.contains(&SUMMARY_TAG.to_string()));
        assert!(names.contains(&TRUNCATED_TAG.to_string()), "nothing records the truncation");
    }

    /// And regenerating it successfully must take the warning away again,
    /// otherwise the first bad run marks the note for good.
    #[test]
    fn a_finished_summary_clears_the_warning() {
        let tags = summary_fields("A whole answer.", false)["tags"].clone();
        let names: Vec<String> =
            tags.as_array().unwrap().iter().map(|t| t["tag"].as_str().unwrap().into()).collect();
        assert_eq!(names, vec![SUMMARY_TAG.to_string()], "a stale warning would stick forever");
    }

    /// The create path builds its draft from the same value, so the fields
    /// have to survive the round trip into `ItemDraft` rather than being
    /// silently dropped by a shape mismatch.
    /// A new summary and a replaced one must produce the same note.
    ///
    /// They are built by different code — a draft cannot be deserialised from
    /// the patch, since `ItemDraft` requires `itemType` — so the only thing
    /// keeping them together is that they share the derivations. This is the
    /// check that says so.
    #[test]
    fn creating_and_replacing_agree() {
        let draft = summary_draft("Body text here.", true);
        let patch = summary_fields("Body text here.", true);
        assert_eq!(draft.fields.get("note").and_then(|v| v.as_str()), Some("Body text here."));
        assert_eq!(draft.fields.get("title"), patch["fields"].get("title"));
        assert_eq!(serde_json::to_value(&draft.tags).unwrap(), patch["tags"]);
        assert_eq!(draft.item_type, "note");
    }
}
