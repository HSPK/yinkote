//! Reading one paper closely.
//!
//! Distinct from a summary, and the difference is the point. A summary answers
//! "should I read this?" in three sentences. A close reading answers "what did
//! they actually do?" — the claim, the method, the evidence, and what the
//! paper does not establish — and it is only honest if the model has read the
//! paper rather than its abstract. So this refuses when the text cannot be
//! read, where `summarise` falls back and says it fell back.
//!
//! The result is a note child, for the same reason a summary is: it belongs to
//! the item, and being a note it is searchable, exportable and syncable with
//! no new machinery.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use yk_ai::ChatMessage;
use yk_core::event::DomainEvent;
use yk_core::model::{Item, ItemDraft};
use yk_core::Error;

use super::summarise::{generated_note_tags, generated_note_title, is_generated_note, Language};
use super::{announce, key};
use crate::error::ApiResult;
use crate::state::App;

/// Marks the note as this feature's, so re-reading replaces rather than piles.
pub const READING_TAG: &str = "close-reading";

pub fn router() -> Router<App> {
    Router::new().route("/libraries/:lib/items/:key/close-reading", post(close_reading))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct ReadingBody {
    /// A question to answer alongside the standard sections. Optional.
    focus: Option<String>,
    language: Option<String>,
}

async fn close_reading(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Json(body): Json<ReadingBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let parent = key(&k)?;
    let item = app.store().items.get(lib, &parent).await?;

    let agent = app.agent().ok_or_else(|| {
        Error::invalid("no model is configured; set agent.endpoint and agent.model")
    })?;

    // Refused rather than answered from the abstract. A close reading of an
    // abstract is a fabrication with headings on it, and it would be filed
    // beside real ones with no way to tell them apart.
    let text = crate::paper::require(&app, lib, &item).await?;
    let language = Language::parse(body.language.as_deref());

    // Registered as a job, because it is one: reading a paper in full can take
    // a model a minute or more, and until now the only sign it was happening
    // was a request that had not come back. The jobs surface exists to answer
    // "what is this server busy with", and the longest thing it does was the
    // one thing missing from it.
    //
    // The request still waits for the answer — the caller wants the note — so
    // this is about visibility and cancellation, not about changing who waits.
    let task = app.tasks().start("close-reading", "task.readingClosely");
    task.progress("task.readingClosely", 0, 1);

    let turn = match agent
        .run(
            lib,
            vec![ChatMessage::new(
                "user",
                prompt(&item, body.focus.as_deref(), language, &text),
            )],
        )
        .await
    {
        Ok(turn) => turn,
        Err(e) => {
            app.tasks().fail(&task, &e);
            return Err(e.into());
        }
    };

    if turn.reply.trim().is_empty() {
        let e = Error::internal("the model returned nothing");
        app.tasks().fail(&task, &e);
        return Err(e.into());
    }

    let note = match save(app.store(), lib, &parent, &turn.reply, turn.truncated).await {
        Ok(note) => note,
        Err(e) => {
            app.tasks().fail(&task, &e);
            return Err(e.into());
        }
    };
    app.tasks().finish(&task, json!({ "note": note.key.as_str() }));

    announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![parent.clone(), note.key.clone()],
        version,
    })
    .await?;

    Ok(Json(json!({
        "note": note,
        "model": agent.model(),
        "truncated": turn.truncated,
        // How much of the paper it saw, so a reading of the first hundred
        // pages of a thesis does not read as a reading of the thesis.
        "charsRead": text.text.chars().count(),
        "partial": text.truncated,
    })))
}

/// One close reading per item: re-reading replaces it.
async fn save(
    store: &yk_store::Store,
    lib: i64,
    parent: &yk_core::Key,
    reply: &str,
    truncated: bool,
) -> Result<Item, Error> {
    let existing = store
        .items
        .children(lib, parent)
        .await?
        .into_iter()
        .find(|c| is_generated_note(c, READING_TAG));
    let title = generated_note_title(existing.as_ref(), "Close reading");
    let tags = generated_note_tags(existing.as_ref(), READING_TAG, truncated);

    match existing.as_ref() {
        Some(note) => {
            let patch = yk_core::model::ItemPatch {
                fields: Some(
                    [("note".to_string(), json!(reply)), ("title".to_string(), json!(title))]
                        .into_iter()
                        .collect(),
                ),
                tags: Some(tags),
                ..Default::default()
            };
            store.items.update(lib, &note.key, patch, Some(note.version)).await
        }
        None => {
            let mut draft = ItemDraft::new("note")
                .with_field("note", reply)
                .with_field("title", title);
            draft.tags = tags;
            draft.parent_key = Some(parent.clone());
            store.items.create(lib, draft).await
        }
    }
}

/// What the model is asked.
///
/// Named sections rather than "analyse this paper", because an unstructured
/// answer to that question is a longer summary. The sections are the questions
/// somebody actually has after reading: what is claimed, how it was tested,
/// what the numbers were, and what the paper does *not* show — the last being
/// the one a model will skip unless asked, and the one worth most.
fn prompt(item: &Item, focus: Option<&str>, language: Language, text: &yk_pdf::Extracted) -> String {
    let mut out = String::from(
        "Read this paper closely for a researcher who will cite it. Use these headings, in \
         this order, and keep each to a short paragraph or a few bullets:\n\
         ## Claim — what the paper argues, in one or two sentences.\n\
         ## Method — what was actually done. Name the data, the model or the proof technique, \
         and the scale.\n\
         ## Evidence — the results that support the claim, with the numbers as reported.\n\
         ## Limitations — what the paper does not establish. Include what the authors admit \
         and what they pass over. If you can see none, say so rather than inventing one.\n\
         ## Relevance — who should read this and why.\n\n\
         Quote sparingly and mark quotations. Do not state anything the text does not support: \
         if the paper is unclear on a point, write that it is unclear.\n",
    );
    out.push_str(language.instruction());

    out.push_str("\n\nTitle: ");
    out.push_str(item.title());
    let creators: Vec<String> = item.creators.iter().map(|c| c.display()).collect();
    if !creators.is_empty() {
        out.push_str(&format!("\nAuthors: {}", creators.join(", ")));
    }

    if let Some(focus) = focus.map(str::trim).filter(|f| !f.is_empty()) {
        out.push_str(&format!("\n\nAlso answer, under a final heading: {focus}"));
    }

    out.push_str("\n\n");
    if text.truncated {
        // Said plainly, because a model given the first half of a paper will
        // otherwise write about its conclusions as though it had seen them.
        out.push_str(
            "This is the beginning of the paper only; it is longer than what follows. Do not \
             describe conclusions you have not been shown.\n",
        );
    }
    out.push_str("--- paper ---\n");
    out.push_str(&text.text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::{Creator, Fields};
    use yk_core::Key;

    fn item() -> Item {
        let mut fields = Fields::new();
        fields.insert("title".into(), json!("Attention Is All You Need"));
        Item {
            key: Key::generate(),
            library_id: 1,
            item_type: "journalArticle".into(),
            parent_key: None,
            fields,
            creators: vec![Creator {
                last_name: Some("Vaswani".into()),
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

    fn text(body: &str, truncated: bool) -> yk_pdf::Extracted {
        yk_pdf::Extracted {
            text: body.into(),
            truncated,
            total_chars: body.chars().count(),
        }
    }

    #[tokio::test]
    async fn generated_close_reading_defaults_and_regeneration_preserve_note_identity() {
        let store = yk_store::Store::in_memory().unwrap();
        let lib = store.default_library;
        let parent = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let first = save(&store, lib, &parent.key, "Close reading", true).await.unwrap();
        assert_eq!(first.item_type, "note");
        assert_eq!(first.title(), "Close reading");
        assert!(first.tags.iter().all(|tag| tag.r#type == 1));
        let next = save(&store, lib, &parent.key, "## Claim\nA new claim.", false).await.unwrap();
        assert_eq!(next.key, first.key);
        assert_eq!(next.parent_key, Some(parent.key));
        assert_eq!(next.title(), "Close reading");
        assert_eq!(next.field("note"), Some("## Claim\nA new claim."));
        assert_eq!(next.tags, generated_note_tags(None, READING_TAG, false));
        assert_eq!(json!(store.items.get(lib, &next.key).await.unwrap()), json!(next));
    }

    #[tokio::test]
    async fn generated_close_reading_only_migrates_legacy_titles_and_keeps_custom_titles() {
        let store = yk_store::Store::in_memory().unwrap();
        let lib = store.default_library;
        for old_body in [
            "## Claim\n\nThe original claim.",
            "<h2>Claim &amp; evidence</h2><p>Old findings.</p>",
            &"精读论文的结论".repeat(20),
        ] {
            for custom in [false, true] {
                let parent =
                    store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
                let legacy = yk_core::text::note_title(old_body, yk_core::text::NOTE_TITLE_CHARS);
                let title = if custom { "My detailed reading" } else { &legacy };
                let mut draft =
                    ItemDraft::new("note").with_field("note", old_body).with_field("title", title);
                draft.parent_key = Some(parent.key.clone());
                draft.tags = generated_note_tags(None, READING_TAG, true);
                draft.tags.push(yk_core::model::ItemTag { tag: "keep".into(), r#type: 0 });
                let original = store.items.create(lib, draft).await.unwrap();
                let next = save(&store, lib, &parent.key, "## Claim\nNew findings.", false)
                    .await
                    .unwrap();
                assert_eq!(next.key, original.key);
                assert_eq!(next.title(), if custom { title } else { "Close reading" });
                assert_eq!(next.field("note"), Some("## Claim\nNew findings."));
                assert!(next.tags.iter().any(|tag| tag.tag == "keep" && tag.r#type == 0));
                assert!(!next.tags.iter().any(|tag| tag.tag == super::super::summarise::TRUNCATED_TAG));
            }
        }
    }

    #[tokio::test]
    async fn generated_close_reading_does_not_replace_a_manually_tagged_note() {
        let store = yk_store::Store::in_memory().unwrap();
        let lib = store.default_library;
        let parent = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let mut draft = ItemDraft::new("note").with_field("note", "My reading");
        draft.parent_key = Some(parent.key.clone());
        draft.tags = vec![yk_core::model::ItemTag { tag: READING_TAG.into(), r#type: 0 }];
        let manual = store.items.create(lib, draft).await.unwrap();
        let reading = save(&store, lib, &parent.key, "Generated.", false).await.unwrap();
        assert_ne!(reading.key, manual.key);
        assert_eq!(json!(store.items.get(lib, &manual.key).await.unwrap()), json!(manual));
    }

    #[test]
    fn the_paper_itself_is_what_is_read() {
        let got = prompt(&item(), None, Language::English, &text("We train on eight GPUs.", false));
        assert!(got.contains("We train on eight GPUs."));
        assert!(got.contains("Attention Is All You Need"));
        assert!(got.contains("Vaswani"));
    }

    /// The heading a model omits unless asked, and the one worth most to
    /// somebody deciding whether to cite.
    #[test]
    fn limitations_are_asked_for_explicitly() {
        let got = prompt(&item(), None, Language::English, &text("body", false));
        for heading in ["## Claim", "## Method", "## Evidence", "## Limitations", "## Relevance"] {
            assert!(got.contains(heading), "{heading} was not asked for");
        }
        assert!(got.contains("say so rather than inventing one"));
    }

    /// A model shown half a paper will write about its conclusions unless the
    /// cut is stated.
    #[test]
    fn a_partial_paper_says_so_in_the_prompt() {
        let whole = prompt(&item(), None, Language::English, &text("body", false));
        let part = prompt(&item(), None, Language::English, &text("body", true));
        assert!(part.contains("beginning of the paper only"));
        assert!(!whole.contains("beginning of the paper only"));
    }

    #[test]
    fn a_focus_becomes_an_extra_heading_and_a_blank_one_does_not() {
        let got = prompt(&item(), Some("does it scale?"), Language::English, &text("b", false));
        assert!(got.contains("does it scale?"));
        assert!(!prompt(&item(), Some("  "), Language::English, &text("b", false))
            .contains("Also answer"));
    }

    #[test]
    fn the_language_reaches_the_prompt() {
        assert!(prompt(&item(), None, Language::Chinese, &text("b", false)).contains("用中文写"));
    }
}
