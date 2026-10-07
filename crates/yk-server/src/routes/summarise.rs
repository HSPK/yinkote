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
use yk_core::model::{Item, ItemDraft, ItemTag};
use yk_ai::ChatMessage;
use yk_core::Error;

use super::{announce, key};
use crate::error::ApiResult;
use crate::state::App;

/// Internal automatic marker used to find this feature's note on regeneration.
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
    generated_note_tags(None, SUMMARY_TAG, truncated)
}

pub(super) fn generated_note_tags(
    existing: Option<&Item>,
    marker: &str,
    truncated: bool,
) -> Vec<ItemTag> {
    let mut tags: Vec<_> = existing
        .into_iter()
        .flat_map(|note| &note.tags)
        .filter(|tag| {
            !(tag.r#type == AUTOMATIC && (tag.tag == marker || tag.tag == TRUNCATED_TAG))
        })
        .cloned()
        .collect();
    tags.push(ItemTag { tag: marker.into(), r#type: AUTOMATIC });
    if truncated {
        tags.push(ItemTag { tag: TRUNCATED_TAG.into(), r#type: AUTOMATIC });
    }
    tags
}

pub(super) fn is_generated_note(note: &Item, marker: &str) -> bool {
    note.item_type == "note"
        && note.tags.iter().any(|tag| tag.tag == marker && tag.r#type == AUTOMATIC)
}

/// Only an absent title or the old body's exact auto-title is replaced.
pub(super) fn generated_note_title<'a>(existing: Option<&'a Item>, default: &'a str) -> &'a str {
    if let Some(note) = existing {
        let title = note.title();
        let legacy = yk_core::text::note_title(
            note.field("note").unwrap_or_default(),
            yk_core::text::NOTE_TITLE_CHARS,
        );
        if !title.trim().is_empty() && title != legacy {
            return title;
        }
    }
    default
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
/// A legacy prose-derived title becomes the default; a custom title stays.
/// Regenerating a truncated summary successfully clears its internal warning.
fn summary_fields(existing: Option<&Item>, reply: &str, truncated: bool) -> serde_json::Value {
    json!({
        "fields": { "note": reply, "title": generated_note_title(existing, "Summary") },
        "tags": generated_note_tags(existing, SUMMARY_TAG, truncated),
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
        .with_field("title", "Summary");
    draft.tags = summary_tags(truncated);
    draft
}

pub fn router() -> Router<App> {
    Router::new().route("/libraries/:lib/items/:key/summarise", post(summarise))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct SummariseBody {
    /// What to emphasise, e.g. "focus on the method". Optional.
    focus: Option<String>,
    /// Which language to write in. Absent means the workbench's own.
    language: Option<String>,
}

/// The language a summary is written in.
///
/// Named rather than free text so the prompt cannot be steered through this
/// field, and so the two the product actually supports are the two on offer.
/// A researcher reading in Chinese wants the summary in Chinese; the same
/// person may want English for a paper they will quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Chinese,
    /// Both, one after the other, for somebody who reads in both.
    Both,
}

impl Language {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).unwrap_or_default().to_lowercase().as_str() {
            "zh" | "zh-cn" | "chinese" => Language::Chinese,
            "both" | "bilingual" => Language::Both,
            _ => Language::English,
        }
    }

    /// What the model is told. Explicit even for English, because a model
    /// given a Chinese paper will otherwise answer in Chinese.
    pub fn instruction(self) -> &'static str {
        match self {
            Language::English => "Write in English, whatever language the paper is in.",
            Language::Chinese => "用中文写，无论论文是什么语言。",
            Language::Both => {
                "Write it twice: first in English under a line reading `## English`, then the                  same summary in Chinese under a line reading `## 中文`. Do not translate word                  for word -- write each so it reads naturally on its own."
            }
        }
    }
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

    // The paper itself when the library holds it. Everything here used to be
    // written from the abstract, which is already a summary -- so the result
    // was a summary of a summary, and read like one.
    let paper = crate::paper::read(&app, lib, &item).await;
    let language = Language::parse(body.language.as_deref());

    // A job for the same reason a close reading is one: it reads the whole
    // paper and waits on a model, and the jobs surface should be able to say
    // that is what the server is doing.
    let task = app.tasks().start("summarise", "task.summarising").await?;
    task.progress("task.summarising", 0, 1);

    let turn = match agent
        .run(
            lib,
            vec![ChatMessage::new(
                "user",
                prompt(&item, body.focus.as_deref(), language, &paper.material(&item)),
            )],
        )
        .await
    {
        Ok(turn) => turn,
        Err(e) => {
            app.tasks().fail(&task, &e).await;
            return Err(e.into());
        }
    };

    if turn.reply.trim().is_empty() {
        let e = Error::internal("the model returned nothing");
        app.tasks().fail(&task, &e).await;
        return Err(e.into());
    }

    let note = match save(app.store(), lib, &parent, &turn.reply, turn.truncated).await {
        Ok(note) => note,
        Err(e) => {
            app.tasks().fail(&task, &e).await;
            return Err(e.into());
        }
    };

    announce(&app, lib, |version| DomainEvent::ItemsChanged {
        library_id: lib,
        keys: vec![parent.clone(), note.key.clone()],
        version,
    })
    .await?;

    app.tasks().finish(&task, json!({ "note": note.key.as_str() })).await;

    Ok(Json(json!({
        "note": note,
        "model": agent.model(),
        "truncated": turn.truncated,
        // Whether it read the paper or only its abstract. The difference is
        // the difference between a summary worth keeping and a paraphrase.
        "readInFull": paper.read_in_full(),
    })))
}

/// One summary per item: regenerate the automatic note, never a user-tagged one.
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
        .find(|c| is_generated_note(c, SUMMARY_TAG));

    match existing {
        Some(note) => {
            store
                .items
                .update(
                    lib,
                    &note.key,
                    serde_json::from_value(summary_fields(Some(&note), reply, truncated))
                        .map_err(internal)?,
                    // A concurrent rename must not be overwritten by this snapshot.
                    Some(note.version),
                )
                .await
        }
        None => {
            let mut draft = summary_draft(reply, truncated);
            draft.parent_key = Some(parent.clone());
            store.items.create(lib, draft).await
        }
    }
}

/// What the model is asked.
///
/// The metadata is handed over directly rather than left for the agent to look
/// up: it already has the item, and a tool round-trip to fetch what the caller
/// is holding is latency for nothing. Searching the library is still available
/// for context the item itself does not carry.
fn prompt(item: &Item, focus: Option<&str>, language: Language, material: &str) -> String {
    let mut out = String::from(
        "Summarise this item for a researcher's own notes. Three or four sentences: what it \
         does, how, and why it matters. Do not repeat the title. Do not invent findings that \
         are not in the material given. If the material is too thin to summarise honestly, \
         say so in one sentence instead of padding.\n",
    );
    out.push_str(language.instruction());
    out.push_str("\n\n");

    let creators: Vec<String> = item.creators.iter().map(|c| c.display()).collect();
    for (label, value) in [
        ("Title", item.title()),
        ("Type", item.item_type.as_str()),
        ("Date", item.field("date").unwrap_or_default()),
        ("Publication", item.field("publicationTitle").unwrap_or_default()),
        ("DOI", item.field("DOI").unwrap_or_default()),
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
    // Last, and labelled by the caller as full text or abstract, because the
    // model must not describe an abstract as though it had read the paper.
    if !material.is_empty() {
        out.push('\n');
        out.push_str(material);
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

    /// What `paper::read` hands over when there is no file: the abstract,
    /// labelled as the abstract.
    const ABSTRACT: &str = "Abstract only -- the full text is not in the library, so do not \
                            claim to have read the paper:\nWe propose the Transformer.";

    #[test]
    fn the_prompt_carries_the_metadata_rather_than_making_the_agent_fetch_it() {
        let text = prompt(&item(), None, Language::English, ABSTRACT);
        assert!(text.contains("Attention Is All You Need"));
        assert!(text.contains("We propose the Transformer."));
        assert!(text.contains("Vaswani"));
    }

    #[test]
    fn empty_fields_are_left_out_rather_than_sent_blank() {
        // "DOI: " teaches a model that blanks are normal and invites it to fill
        // them in.
        let text = prompt(&item(), None, Language::English, ABSTRACT);
        assert!(!text.contains("DOI:"));
        assert!(!text.contains("Publication:"));
    }

    #[test]
    fn a_focus_is_passed_through_but_a_blank_one_is_not() {
        assert!(prompt(&item(), Some("the method"), Language::English, ABSTRACT).contains("focus on: the method"));
        assert!(!prompt(&item(), Some("   "), Language::English, ABSTRACT).contains("focus on"));
        assert!(!prompt(&item(), None, Language::English, ABSTRACT).contains("focus on"));
    }

    #[test]
    fn the_model_is_told_not_to_pad_a_thin_record() {
        assert!(prompt(&item(), None, Language::English, ABSTRACT).contains("too thin"));
    }

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
            serde_json::from_value(summary_fields(None, "Evaluates on three benchmarks.", false))
                .unwrap();
        let fields = patch.fields.expect("the patch changes no fields at all");
        assert_eq!(
            fields.get("note").and_then(|v| v.as_str()),
            Some("Evaluates on three benchmarks."),
        );
        assert_eq!(fields.get("title").and_then(|v| v.as_str()), Some("Summary"));
        assert!(patch.tags.is_some(), "the truncation warning would never be cleared");
    }

    /// A note outlives the toast that announced it.
    #[test]
    fn an_unfinished_summary_says_so_on_the_note() {
        let tags = summary_fields(None, "Half an ans", true)["tags"].clone();
        let names: Vec<String> =
            tags.as_array().unwrap().iter().map(|t| t["tag"].as_str().unwrap().into()).collect();
        assert!(names.contains(&SUMMARY_TAG.to_string()));
        assert!(names.contains(&TRUNCATED_TAG.to_string()), "nothing records the truncation");
    }

    /// And regenerating it successfully must take the warning away again,
    /// otherwise the first bad run marks the note for good.
    #[test]
    fn a_finished_summary_clears_the_warning() {
        let tags = summary_fields(None, "A whole answer.", false)["tags"].clone();
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
        let patch = summary_fields(None, "Body text here.", true);
        assert_eq!(draft.fields.get("note").and_then(|v| v.as_str()), Some("Body text here."));
        assert_eq!(draft.fields.get("title"), patch["fields"].get("title"));
        assert_eq!(serde_json::to_value(&draft.tags).unwrap(), patch["tags"]);
        assert_eq!(draft.item_type, "note");
    }

    #[tokio::test]
    async fn generated_summary_defaults_and_regeneration_preserve_note_identity() {
        let store = yk_store::Store::in_memory().unwrap();
        let lib = store.default_library;
        let parent = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let first = save(&store, lib, &parent.key, "Summary", true).await.unwrap();
        assert_eq!(first.item_type, "note");
        assert_eq!(first.title(), "Summary");
        assert!(first.tags.iter().all(|tag| tag.r#type == AUTOMATIC));

        let next = save(&store, lib, &parent.key, "A different body.", false).await.unwrap();
        assert_eq!(next.key, first.key);
        assert_eq!(next.parent_key, Some(parent.key));
        assert_eq!(next.title(), "Summary", "a body matching the old title cannot rename it");
        assert_eq!(next.field("note"), Some("A different body."));
        assert_eq!(next.tags, summary_tags(false));
        assert_eq!(
            json!(store.items.get(lib, &next.key).await.unwrap()),
            json!(next),
        );
    }

    #[tokio::test]
    async fn generated_summary_only_migrates_legacy_titles_and_keeps_custom_titles() {
        let store = yk_store::Store::in_memory().unwrap();
        let lib = store.default_library;
        for old_body in [
            "## English\n\nThe original summary.\n\n## 中文\n原来的总结。",
            "<p>Q &amp; A <b>summary</b></p><p>Details.</p>",
            &"原来的总结".repeat(20),
        ] {
            for custom in [false, true] {
                let parent =
                    store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
                let legacy = yk_core::text::note_title(old_body, yk_core::text::NOTE_TITLE_CHARS);
                let title = if custom { "My research summary" } else { &legacy };
                let mut draft = summary_draft(old_body, true).with_field("title", title);
                draft.parent_key = Some(parent.key.clone());
                draft.tags.push(ItemTag { tag: "keep this tag".into(), r#type: 0 });
                let original = store.items.create(lib, draft).await.unwrap();
                let next = save(&store, lib, &parent.key, "New summary.", false).await.unwrap();
                assert_eq!(next.key, original.key);
                assert_eq!(next.title(), if custom { title } else { "Summary" });
                assert_eq!(next.field("note"), Some("New summary."));
                assert!(next.tags.iter().any(|tag| tag.tag == "keep this tag" && tag.r#type == 0));
                assert!(!next.tags.iter().any(|tag| tag.tag == TRUNCATED_TAG));
            }
        }
    }

    #[tokio::test]
    async fn generated_summary_does_not_replace_a_manually_tagged_note() {
        let store = yk_store::Store::in_memory().unwrap();
        let lib = store.default_library;
        let parent = store.items.create(lib, ItemDraft::new("journalArticle")).await.unwrap();
        let mut draft = ItemDraft::new("note").with_field("note", "My own note");
        draft.parent_key = Some(parent.key.clone());
        draft.tags = vec![ItemTag { tag: SUMMARY_TAG.into(), r#type: 0 }];
        let manual = store.items.create(lib, draft).await.unwrap();
        let summary = save(&store, lib, &parent.key, "Generated.", false).await.unwrap();
        assert_ne!(summary.key, manual.key);
        assert_eq!(json!(store.items.get(lib, &manual.key).await.unwrap()), json!(manual));
    }

    /// The paper, not its abstract. Everything here was written from the
    /// abstract, which is already a summary.
    #[test]
    fn the_full_text_is_what_the_model_is_given_when_the_library_has_it() {
        let full = "Full text of the paper:\nSection 1. We train on eight GPUs for twelve hours.";
        let text = prompt(&item(), None, Language::English, full);
        assert!(text.contains("eight GPUs"), "the paper itself must reach the model");
        // And the metadata no longer repeats the abstract, which arrives as
        // material or not at all.
        assert!(!text.contains("Abstract:"));
    }

    /// A model handed a Chinese paper answers in Chinese unless told, so every
    /// choice is stated -- including the default.
    #[test]
    fn the_language_is_always_stated() {
        for language in [Language::English, Language::Chinese, Language::Both] {
            let text = prompt(&item(), None, language, ABSTRACT);
            assert!(!text.is_empty());
            assert!(
                text.contains("English") || text.contains("中文"),
                "{language:?} said nothing about language"
            );
        }
        assert!(prompt(&item(), None, Language::Chinese, ABSTRACT).contains("用中文写"));
        let both = prompt(&item(), None, Language::Both, ABSTRACT);
        assert!(both.contains("## English") && both.contains("## 中文"));
    }

    #[test]
    fn a_language_is_read_from_the_names_a_client_would_send() {
        assert_eq!(Language::parse(Some("zh-CN")), Language::Chinese);
        assert_eq!(Language::parse(Some("Chinese")), Language::Chinese);
        assert_eq!(Language::parse(Some("both")), Language::Both);
        assert_eq!(Language::parse(Some("en")), Language::English);
        // Anything unrecognised is English rather than an error: a summary in
        // the wrong language beats no summary.
        assert_eq!(Language::parse(Some("klingon")), Language::English);
        assert_eq!(Language::parse(None), Language::English);
    }
}
