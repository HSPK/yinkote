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
                let mut out = String::from("Full text of the paper");
                if got.truncated {
                    out.push_str(" (the first part only; it is longer than this)");
                }
                out.push_str(":\n");
                out.push_str(&got.text);
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
