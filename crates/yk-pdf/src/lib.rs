//! Reading the text out of a PDF.
//!
//! The library already stores the paper; until now nothing on the server could
//! read it, so every AI feature worked from the abstract — which is the one
//! part of a paper that is already a summary. Summarising a summary is not
//! close reading, and the difference is visible in the answer.
//!
//! Extraction is best-effort by nature. A PDF may be a scan with no text layer
//! at all, and the honest answer there is "nothing", not an error: the caller
//! decides whether to fall back to the abstract.

pub mod pipeline;

pub use pipeline::{External, Mode, Pipeline};

use yk_core::{Error, Result};

/// How much text one paper contributes.
///
/// A long thesis can run to a million characters, and the point of a limit is
/// that a model has a context and a reader has a wait. Two hundred thousand is
/// roughly a hundred pages of prose — past the length of anything that is
/// usefully read in one pass.
pub const MAX_CHARS: usize = 200_000;

/// What a PDF turned out to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// The text, normalised and bounded by [`MAX_CHARS`].
    pub text: String,
    /// True when [`MAX_CHARS`] cut it short, so a caller can say so.
    pub truncated: bool,
    /// Characters before truncation, which is the honest length of the paper.
    pub total_chars: usize,
}

impl Extracted {
    /// Whether there is enough here to be worth reading.
    ///
    /// A scanned paper yields a handful of stray glyphs rather than nothing at
    /// all, and a caller that tests `is_empty()` would treat that as success
    /// and feed the model noise.
    pub fn is_useful(&self) -> bool {
        self.text.chars().filter(|c| c.is_alphanumeric()).count() >= 200
    }
}

/// Read the text out of PDF bytes.
///
/// Runs on the calling thread and is CPU-bound; callers on an async runtime
/// should wrap it in `spawn_blocking`.
pub fn extract(bytes: &[u8]) -> Result<Extracted> {
    if bytes.is_empty() {
        return Err(Error::invalid("that file is empty"));
    }
    // `pdf-extract` panics on some malformed files rather than returning an
    // error. A paper somebody downloaded is exactly the kind of file that is
    // subtly malformed, and taking the server down with it is not an option.
    let raw = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| Error::invalid("that file could not be read as a PDF"))?
        .map_err(|e| Error::invalid(format!("that file could not be read as a PDF: {e}")))?;

    Ok(bound(&normalise(&raw)))
}

/// Collapse the whitespace a PDF's layout leaves behind.
///
/// Text extracted from a two-column paper arrives with a line break every
/// forty characters and blank lines between them. Left alone that triples the
/// token count and reads to a model as a poem.
pub(crate) fn normalise(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blank_run = 0usize;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            blank_run += 1;
            // One blank line is a paragraph break and worth keeping; a run of
            // them is the space between columns.
            if blank_run == 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank_run = 0;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        // A word broken across a line break is one word.
        match out.strip_suffix("- ") {
            Some(joined) if line.starts_with(|c: char| c.is_lowercase()) => {
                out = joined.to_string();
            }
            _ => {}
        }
        out.push_str(line);
    }
    out
}

pub(crate) fn bound(text: &str) -> Extracted {
    let total_chars = text.chars().count();
    match text.char_indices().nth(MAX_CHARS) {
        Some((cut, _)) => {
            Extracted { text: text[..cut].to_string(), truncated: true, total_chars }
        }
        None => Extracted { text: text.to_string(), truncated: false, total_chars },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_refused_rather_than_read_as_blank() {
        // Silence here would look like a scanned paper, and the caller would
        // fall back instead of saying the file is broken.
        assert!(extract(&[]).is_err());
    }

    #[test]
    fn something_that_is_not_a_pdf_is_an_error_and_not_a_panic() {
        assert!(extract(b"<html><body>not a pdf</body></html>").is_err());
    }

    #[test]
    fn the_layout_of_a_column_is_not_kept() {
        // A two-column paper extracts one short line per row of type.
        let raw = "We present a\nmethod for reading\n\n\n\npapers that are\nlong.";
        assert_eq!(normalise(raw), "We present a method for reading\npapers that are long.");
    }

    #[test]
    fn a_word_broken_across_lines_is_rejoined() {
        assert_eq!(normalise("trans-\nformer models"), "transformer models");
        // But a real hyphen is not a line break artefact.
        assert_eq!(normalise("state-of-the-art results"), "state-of-the-art results");
        // And a capital after a hyphen is a compound, not a split word.
        assert_eq!(normalise("Bert-\nStyle models"), "Bert- Style models");
    }

    #[test]
    fn a_long_paper_is_cut_and_says_so() {
        let long = "a".repeat(MAX_CHARS + 500);
        let got = bound(&long);
        assert!(got.truncated);
        assert_eq!(got.text.chars().count(), MAX_CHARS);
        assert_eq!(got.total_chars, MAX_CHARS + 500, "the honest length, not the cut one");
    }

    #[test]
    fn a_scan_with_a_few_stray_glyphs_is_not_useful() {
        // The failure this guards: `is_empty()` is false for the handful of
        // characters a scanned page yields, so a caller testing emptiness
        // feeds the model noise and calls it the paper.
        let scan = bound("f1 3 . 2\n\u{c}");
        assert!(!scan.text.is_empty());
        assert!(!scan.is_useful());

        let real = bound(&"the transformer architecture ".repeat(20));
        assert!(real.is_useful());
    }
}
