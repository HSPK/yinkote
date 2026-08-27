//! Turning what somebody highlighted into something they can read.
//!
//! A paper's annotations live scattered across its attachments, one item each,
//! in the order they were drawn. What a person wants at the end of reading is
//! all of it in one place, in page order, with their own comments beside the
//! passages that prompted them — and they want it as a *note*, because a note
//! is searchable, exportable and editable, and a highlight is none of those.
//!
//! The rendering is pure and lives here so it can be tested without a PDF, a
//! store or a request.

use serde::Serialize;
use yk_core::model::Item;

/// One highlight or comment, as the note needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Annotation {
    pub key: String,
    /// `highlight`, `underline`, `note`, `image`…
    pub kind: String,
    /// The passage marked in the PDF. Empty for a standalone sticky note.
    pub text: String,
    /// What the reader wrote about it.
    pub comment: String,
    pub colour: String,
    pub page: i64,
}

impl Annotation {
    /// Read one out of the item it is stored as, or `None` if it is not one.
    pub fn of(item: &Item) -> Option<Self> {
        if item.item_type != "annotation" {
            return None;
        }
        let field = |name: &str| item.field(name).unwrap_or_default().trim().to_string();
        Some(Self {
            key: item.key.to_string(),
            kind: {
                let k = field("annotationType");
                if k.is_empty() {
                    "highlight".to_string()
                } else {
                    k
                }
            },
            text: field("annotationText"),
            comment: field("annotationComment"),
            colour: field("annotationColor"),
            // A page that will not parse sorts first rather than being dropped:
            // losing somebody's highlight is worse than putting it in the wrong
            // place, and they can see where it went.
            page: field("annotationPage").parse().unwrap_or(0),
        })
    }
}

/// Render annotations as the HTML body of a note.
///
/// Page order, then the order they were drawn, because that is reading order —
/// the order the items happen to come back in is not anything.
///
/// An annotation with neither a passage nor a comment is dropped: it is a mark
/// on a page with nothing in it, and a bullet saying nothing is worse than one
/// fewer bullet.
pub fn render(title: &str, annotations: &[Annotation]) -> String {
    let mut kept: Vec<&Annotation> =
        annotations.iter().filter(|a| !a.text.is_empty() || !a.comment.is_empty()).collect();
    kept.sort_by_key(|a| a.page);

    let mut out = String::with_capacity(256 + kept.len() * 160);
    out.push_str(&format!("<h1>{}</h1>\n", escape(title)));

    let mut page = i64::MIN;
    let mut open = false;
    for a in kept {
        if a.page != page {
            if open {
                out.push_str("</ul>\n");
            }
            page = a.page;
            // A page number is what lets somebody go back and check, which is
            // most of what a note like this is for.
            out.push_str(&format!("<h2>p. {page}</h2>\n<ul>\n"));
            open = true;
        }
        out.push_str("<li>");
        if !a.text.is_empty() {
            // Quoted, because it is somebody else's words: a note that mixes
            // the paper's sentences with the reader's own is a plagiarism
            // risk wearing a friendly face.
            out.push_str(&format!("<blockquote>{}</blockquote>", escape(&a.text)));
        }
        if !a.comment.is_empty() {
            out.push_str(&format!("<p>{}</p>", escape(&a.comment)));
        }
        out.push_str("</li>\n");
    }
    if open {
        out.push_str("</ul>\n");
    }
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::ItemDraft;
    use yk_core::Key;

    fn mark(page: i64, text: &str, comment: &str) -> Annotation {
        Annotation {
            key: Key::generate().to_string(),
            kind: "highlight".into(),
            text: text.into(),
            comment: comment.into(),
            colour: "#ffd400".into(),
            page,
        }
    }

    #[test]
    fn reads_in_page_order_whatever_order_they_arrive_in() {
        let out = render(
            "A Paper",
            &[mark(7, "later", ""), mark(2, "earlier", ""), mark(7, "also later", "")],
        );
        let two = out.find("p. 2").unwrap();
        let seven = out.find("p. 7").unwrap();
        assert!(two < seven);
        assert_eq!(out.matches("<h2>").count(), 2, "one heading per page, not per mark");
        assert!(out.find("later").unwrap() < out.find("also later").unwrap());
    }

    #[test]
    fn quotes_the_paper_and_does_not_quote_the_reader() {
        // A note that runs the two together is a plagiarism risk that looks
        // helpful.
        let out = render("A Paper", &[mark(1, "their sentence", "my thought")]);
        assert!(out.contains("<blockquote>their sentence</blockquote>"));
        assert!(out.contains("<p>my thought</p>"));
        assert!(!out.contains("<blockquote>my thought"));
    }

    #[test]
    fn a_comment_with_no_passage_is_still_kept() {
        // A sticky note in the margin has no highlighted text at all.
        let out = render("A Paper", &[mark(3, "", "just a thought")]);
        assert!(out.contains("just a thought"));
        assert!(!out.contains("<blockquote>"));
    }

    #[test]
    fn a_mark_with_nothing_in_it_is_dropped() {
        let out = render("A Paper", &[mark(1, "", ""), mark(2, "kept", "")]);
        assert_eq!(out.matches("<li>").count(), 1);
        assert!(!out.contains("p. 1"));
    }

    #[test]
    fn markup_in_a_paper_does_not_become_markup_in_the_note() {
        // Papers about HTML exist, and a note is rendered as HTML.
        let out = render("<script>", &[mark(1, "a < b && c > d", "")]);
        assert!(!out.contains("<script>"));
        assert!(out.contains("&lt;script&gt;"));
        assert!(out.contains("a &lt; b &amp;&amp; c &gt; d"));
    }

    #[test]
    fn only_annotations_are_read_out_of_items() {
        let note = ItemDraft::new("note").into_item(Key::generate(), 1, 1);
        assert!(Annotation::of(&note).is_none());

        let item = ItemDraft::new("annotation")
            .with_field("annotationText", "  spaced  ")
            .with_field("annotationPage", "12")
            .into_item(Key::generate(), 1, 1);
        let a = Annotation::of(&item).expect("an annotation");
        assert_eq!(a.text, "spaced");
        assert_eq!(a.page, 12);
        assert_eq!(a.kind, "highlight", "a mark with no type is a highlight");
    }

    #[test]
    fn a_page_that_will_not_parse_keeps_its_annotation() {
        // Some importers write "iv" or "12-13". Dropping the highlight would
        // lose the reader's work; putting it first at least shows it.
        let item = ItemDraft::new("annotation")
            .with_field("annotationText", "roman")
            .with_field("annotationPage", "iv")
            .into_item(Key::generate(), 1, 1);
        assert_eq!(Annotation::of(&item).unwrap().page, 0);
    }
}
