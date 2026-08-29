//! The text of a paper, for the features that read it rather than list it.
//!
//! Everything the assistant knew about a paper used to come from its abstract,
//! which is the one part that is already a summary. Summarising a summary
//! produces something that reads like a summary and says nothing new.
//!
//! One place, because summarising and close reading want exactly the same
//! thing and would otherwise disagree about which attachment is "the paper"
//! and what to do when it cannot be read.

use yk_core::model::Item;
use yk_core::{Key, Result};

use crate::state::App;

/// How much of a paper a *summary* is written from.
///
/// Not the whole thing. Measured on a 202,432-character paper: the full text
/// costs 47,110 prompt tokens and 5.2 seconds, and 25,000 characters costs
/// 6,198 and 4.8 — eight times the tokens for a summary that says the same
/// thing. On a shared endpoint that limits by tokens, the large request is
/// also eight times likelier to be refused, which is the failure this feature
/// actually has.
///
/// A close reading is not bounded this way: it is asked what the paper *did*,
/// section by section, and the middle is where that lives.
const SUMMARY_CHARS: usize = 25_000;

/// What could be read of one paper.
pub struct Paper {
    /// The extracted text, absent when there is no readable file.
    pub text: Option<yk_pdf::Extracted>,
    /// Which attachment it came from, for saying so.
    pub source: Option<String>,
}

impl Paper {
    /// The material for a prompt: the full text if there is any, otherwise the
    /// abstract, which is better than nothing and worse than the paper.
    ///
    /// Labelled either way. A model told "Abstract:" and a model told "Full
    /// text:" should not answer with the same confidence, and a reader looking
    /// at the result deserves to know which it was.
    pub fn material(&self, item: &Item) -> String {
        match &self.text {
            Some(got) if got.is_useful() => {
                let excerpt = got.excerpt(SUMMARY_CHARS);
                let shortened = excerpt.chars().count() < got.text.chars().count();
                let mut out = String::from("Full text of the paper");
                if got.truncated || shortened {
                    out.push_str(" (its beginning and its end; the middle is omitted)");
                }
                out.push_str(":\n");
                out.push_str(&excerpt);
                out
            }
            _ => match item.field("abstractNote").unwrap_or_default() {
                "" => String::new(),
                abstract_note => format!(
                    "Abstract only -- the full text is not in the library, so do not claim \
                     to have read the paper:\n{abstract_note}"
                ),
            },
        }
    }

    /// Whether the full text was actually read, which callers report.
    pub fn read_in_full(&self) -> bool {
        self.text.as_ref().is_some_and(|t| t.is_useful())
    }
}

/// Read an item's PDF, if it has one that can be read.
///
/// Never an error. A paper with no file, a file that is a scan, and a file
/// that is malformed are all "no text available", and every caller's answer to
/// those three is the same: fall back to the abstract and say so. Turning them
/// into errors would only make each caller re-flatten them.
pub async fn read(app: &App, lib: i64, item: &Item) -> Paper {
    let Some((key, filename)) = pdf_attachment(app, lib, item).await else {
        return Paper { text: None, source: None };
    };

    let bytes = match app.storage().get(&key, &filename).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::debug!(error = %e, %filename, "the paper's file could not be opened");
            return Paper { text: None, source: None };
        }
    };

    // The built-in reader is CPU-bound and takes ~100ms for a real paper,
    // which is long enough to matter to every other request sharing the
    // runtime; the pipeline puts it on a blocking thread and may hand the file
    // to a layout model instead, if one is configured.
    match app.config().pdf.pipeline().read(&bytes).await {
        Ok(text) => Paper { text: Some(text), source: Some(filename) },
        Err(e) => {
            tracing::debug!(error = %e, %filename, "the paper's file could not be read");
            Paper { text: None, source: Some(filename) }
        }
    }
}

/// The attachment that is the paper itself.
///
/// An item accumulates attachments -- supplementary material, a poster, the
/// slides -- and the PDF added first is the one somebody meant by "the paper".
async fn pdf_attachment(app: &App, lib: i64, item: &Item) -> Option<(Key, String)> {
    let children: Vec<Item> = app.store().items.children(lib, &item.key).await.ok()?;
    children
        .into_iter()
        .filter(|c| c.item_type == "attachment")
        .find(|c| {
            c.field("contentType") == Some("application/pdf")
                || c.field("filename").is_some_and(|f| f.to_lowercase().ends_with(".pdf"))
        })
        .and_then(|c| c.field("filename").map(|f| (c.key.clone(), f.to_string())))
}

/// Read an item's text, or say why it cannot be read.
///
/// For the endpoints that would rather refuse than quietly answer from the
/// abstract.
pub async fn require(app: &App, lib: i64, item: &Item) -> Result<yk_pdf::Extracted> {
    let paper = read(app, lib, item).await;
    match paper.text {
        Some(text) if text.is_useful() => Ok(text),
        Some(_) => Err(yk_core::Error::invalid(
            "that file has no text in it -- it is probably a scan, which needs OCR",
        )),
        None => Err(yk_core::Error::invalid("that item has no readable PDF attached")),
    }
}

#[cfg(test)]
mod material_size {
    use super::*;
    use yk_core::model::Fields;

    fn paper(chars: usize) -> Paper {
        let text = "the transformer architecture ".repeat(chars / 29 + 1);
        Paper {
            text: Some(yk_pdf::Extracted {
                total_chars: text.chars().count(),
                truncated: false,
                text,
            }),
            source: Some("paper.pdf".into()),
        }
    }

    fn item() -> Item {
        let mut fields = Fields::new();
        fields.insert("title".into(), serde_json::json!("A paper"));
        fields.insert("abstractNote".into(), serde_json::json!("An abstract."));
        Item {
            key: yk_core::Key::generate(),
            library_id: 1,
            item_type: "journalArticle".into(),
            parent_key: None,
            fields,
            creators: Vec::new(),
            tags: Vec::new(),
            collections: Vec::new(),
            version: 1,
            deleted: false,
            attachments: Vec::new(),
            date_added: 0,
            date_modified: 0,
        }
    }

    /// A hundred pages of prose cost eight times the tokens of twenty-five
    /// thousand characters and produced the same summary -- measured, on a
    /// real paper, at 47,110 tokens against 6,198. On a shared endpoint that
    /// limits by tokens, the large request is also the one that gets refused.
    #[test]
    fn a_long_paper_is_not_sent_whole_to_be_summarised() {
        let long = paper(200_000);
        let material = long.material(&item());
        assert!(
            material.chars().count() < SUMMARY_CHARS + 500,
            "sent {} characters",
            material.chars().count()
        );
        // And it says it is an excerpt, so the model does not describe a
        // conclusion that followed from text it was not shown.
        assert!(material.contains("middle is omitted"));
    }

    /// Most papers are shorter than the budget and must arrive whole.
    #[test]
    fn a_short_paper_is_sent_entire() {
        let short = paper(4_000);
        let material = short.material(&item());
        assert!(!material.contains("middle is omitted"));
        assert!(material.contains("Full text of the paper"));
    }

    /// With no readable file the abstract is used and *labelled* as one, so
    /// the model does not claim to have read the paper.
    #[test]
    fn without_a_file_the_abstract_is_named_as_such() {
        let none = Paper { text: None, source: None };
        let material = none.material(&item());
        assert!(material.contains("Abstract only"));
        assert!(material.contains("do not claim to have read the paper"));
        assert!(!none.read_in_full());
    }
}
