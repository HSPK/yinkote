//! Working out what a document's citations should now say.
//!
//! This is the whole of the hard part, and it is deliberately pure: no store,
//! no session, no HTTP. Given the fields a word processor currently holds, in
//! document order, and the items they cite, it says what each field should now
//! read and what the bibliography should be.
//!
//! Why the add-in sends the whole document every time: in a numeric style —
//! IEEE, GB/T 7714's numeric form — inserting one citation renumbers every
//! citation after it, and deleting one closes the gap. A server shown only the
//! citation being inserted would have to guess at the rest of the paper. So the
//! contract is a snapshot in, a full answer out, and the diff is worked out
//! here rather than in the add-in: the server knows what it just rendered, and
//! asking each client to compare strings would put this logic in every one of
//! them.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use yk_cite::{Format, Style};
use yk_core::model::Item;

/// What one citation field cites.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Citation {
    /// The items cited, in the order the author put them.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Words before the citation, inside the punctuation: "see".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Words after it, inside the punctuation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// A page or a chapter: "p. 41".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

/// One citation field as the document currently has it.
#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    /// The content control's id, which the add-in keeps in its CustomXmlPart.
    pub id: String,
    /// What the document shows now, so unchanged fields can be left alone.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub citation: Citation,
}

/// A field whose text has changed and must be written back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rendered {
    pub id: String,
    pub text: String,
}

/// One bibliography entry, with the key it came from so the add-in can link it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub key: String,
    pub text: String,
}

/// Everything the document should now show.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    /// Only the fields that changed. Writing every field back would dirty the
    /// whole document and throw the author out of their place in it.
    #[serde(rename = "updatedFields")]
    pub updated: Vec<Rendered>,
    pub bibliography: Vec<Entry>,
}

/// Work out the document's new state.
///
/// `items` need only hold the keys the fields mention. A key with no item —
/// deleted from the library since it was cited — keeps whatever the document
/// already said: silently blanking a citation destroys something the author
/// cannot get back, while a stale one is at least visible.
pub fn plan(fields: &[Field], items: &HashMap<String, Item>, style: &Style, format: Format) -> Plan {
    // First appearance decides the number, and the bibliography order for a
    // numeric style. A work cited three times keeps the number it got first.
    //
    // Only keys that resolve are numbered. Numbering a missing one would
    // reserve a slot nothing can fill, and the paper would run [2], [3] with no
    // [1] anywhere — the bibliography cannot list what the library does not
    // have, so the two would disagree.
    let mut order: Vec<String> = Vec::new();
    let mut number: HashMap<&str, usize> = HashMap::new();
    for field in fields {
        for key in &field.citation.keys {
            if !items.contains_key(key.as_str()) || number.contains_key(key.as_str()) {
                continue;
            }
            order.push(key.clone());
            number.insert(key.as_str(), order.len());
        }
    }

    let updated = fields
        .iter()
        .filter_map(|field| {
            let text = render_field(field, items, &number, style)?;
            // The diff. Only what moved.
            (text != field.text).then(|| Rendered { id: field.id.clone(), text })
        })
        .collect();

    Plan { updated, bibliography: bibliography(&order, items, style, format) }
}

/// The text of one field, or `None` when nothing it cites is in the library.
fn render_field(
    field: &Field,
    items: &HashMap<String, Item>,
    number: &HashMap<&str, usize>,
    style: &Style,
) -> Option<String> {
    let bodies: Vec<String> = field
        .citation
        .keys
        .iter()
        .filter_map(|key| {
            let item = items.get(key)?;
            let n = number.get(key.as_str()).copied().unwrap_or(1);
            Some(yk_cite::citation_body(item, style, n))
        })
        .collect();
    if bodies.is_empty() {
        return None;
    }

    // Numbers pack together — `[1,2]`, not `[1], [2]` — because that is what a
    // numeric style does when one sentence cites two papers.
    let joined = bodies.join(if style.numeric { "," } else { "; " });

    let trimmed = |s: &Option<String>| {
        s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
    };

    let mut inner = String::new();
    if let Some(prefix) = trimmed(&field.citation.prefix) {
        inner.push_str(&prefix);
        inner.push(' ');
    }
    inner.push_str(&joined);
    if let Some(locator) = trimmed(&field.citation.locator) {
        inner.push_str(", ");
        inner.push_str(&locator);
    }
    if let Some(suffix) = trimmed(&field.citation.suffix) {
        inner.push(' ');
        inner.push_str(&suffix);
    }

    // The punctuation goes on last, so a locator lands inside it.
    Some(if style.numeric { format!("[{inner}]") } else { format!("({inner})") })
}

/// The bibliography, in the order the style calls for.
fn bibliography(
    order: &[String],
    items: &HashMap<String, Item>,
    style: &Style,
    format: Format,
) -> Vec<Entry> {
    let mut cited: Vec<(&String, &Item)> =
        order.iter().filter_map(|key| items.get(key).map(|item| (key, item))).collect();

    // A numeric style lists entries in the order they were cited, because the
    // numbers have to run 1, 2, 3 down the page. Everything else sorts, and it
    // sorts on the rendered reference — which begins with the author's name —
    // so the order agrees with what the reader sees rather than with some
    // separate idea of the author's name.
    if !style.numeric {
        cited.sort_by_cached_key(|(_, item)| {
            yk_cite::reference(item, style, Format::Text).to_lowercase()
        });
    }

    cited
        .iter()
        .enumerate()
        .map(|(i, (key, item))| {
            let body = yk_cite::reference(item, style, format);
            Entry {
                key: (*key).clone(),
                text: if style.numeric { format!("[{}] {body}", i + 1) } else { body },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::{Creator, ItemDraft};
    use yk_core::Key;

    fn item(surname: &str, year: &str, title: &str) -> Item {
        ItemDraft::new("journalArticle")
            .with_field("title", title)
            .with_field("date", year)
            // A journal name, because a reference without one has nothing to
            // italicise and the HTML assertion would pass on an empty string.
            .with_field("publicationTitle", "Journal of Testing")
            .with_creator(Creator {
                // `Default` leaves this empty, and an empty creator type is not
                // an author — the reference would render with no name on it and
                // every ordering assertion below would be testing nothing.
                creator_type: "author".into(),
                last_name: Some(surname.into()),
                first_name: Some("A".into()),
                name: None,
            })
            .into_item(Key::generate(), 1, 1)
    }

    fn library(pairs: &[(&str, Item)]) -> HashMap<String, Item> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
    }

    fn field(id: &str, text: &str, keys: &[&str]) -> Field {
        Field {
            id: id.into(),
            text: text.into(),
            citation: Citation {
                keys: keys.iter().map(|k| (*k).to_string()).collect(),
                ..Default::default()
            },
        }
    }

    fn two() -> HashMap<String, Item> {
        library(&[
            ("AAA", item("Zhang", "2020", "First")),
            ("BBB", item("Li", "2021", "Second")),
        ])
    }

    fn ieee() -> &'static Style {
        yk_cite::find("ieee").expect("a numeric style")
    }

    fn apa() -> &'static Style {
        yk_cite::find("apa").expect("an author-date style")
    }

    fn text_of(plan: &Plan, id: &str) -> String {
        plan.updated.iter().find(|r| r.id == id).map(|r| r.text.clone()).unwrap_or_default()
    }

    #[test]
    fn inserting_a_citation_renumbers_everything_after_it() {
        // The reason the whole document is sent on every edit. The author has
        // just put a citation to BBB in front of one to AAA; both fields still
        // hold the text that was right a moment ago.
        let fields = vec![field("new", "", &["BBB"]), field("old", "[1]", &["AAA"])];

        let plan = plan(&fields, &two(), ieee(), Format::Text);

        assert_eq!(
            plan.updated,
            vec![
                Rendered { id: "new".into(), text: "[1]".into() },
                Rendered { id: "old".into(), text: "[2]".into() },
            ],
            "the new citation and the one it renumbered"
        );
        assert_eq!(
            plan.bibliography.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["BBB", "AAA"],
            "a numeric bibliography runs in citation order"
        );
    }

    #[test]
    fn a_document_that_is_already_right_asks_for_no_changes() {
        // Refreshing an untouched paper must not dirty a single field.
        let fields = vec![field("f1", "[1]", &["AAA"]), field("f2", "[2]", &["BBB"])];
        let plan = plan(&fields, &two(), ieee(), Format::Text);
        assert!(plan.updated.is_empty());
        assert_eq!(plan.bibliography.len(), 2, "but the bibliography is still returned");
    }

    #[test]
    fn a_key_cited_twice_keeps_its_first_number() {
        let fields =
            vec![field("f1", "", &["AAA"]), field("f2", "", &["BBB"]), field("f3", "", &["AAA"])];

        let plan = plan(&fields, &two(), ieee(), Format::Text);
        assert_eq!(text_of(&plan, "f1"), "[1]");
        assert_eq!(text_of(&plan, "f3"), "[1]", "the same work is the same number");
        assert_eq!(plan.bibliography.len(), 2, "and it is listed once");
    }

    #[test]
    fn one_field_citing_two_works_packs_its_numbers() {
        let fields = vec![field("f1", "", &["AAA", "BBB"])];
        let plan = plan(&fields, &two(), ieee(), Format::Text);
        assert_eq!(plan.updated[0].text, "[1,2]");
    }

    #[test]
    fn an_author_date_bibliography_is_sorted_rather_than_cited_in_order() {
        // Zhang is cited first; Li sorts first.
        let fields = vec![field("f1", "", &["AAA"]), field("f2", "", &["BBB"])];

        let plan = plan(&fields, &two(), apa(), Format::Text);
        assert_eq!(
            plan.bibliography.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["BBB", "AAA"]
        );
        assert_eq!(text_of(&plan, "f1"), "(Zhang, 2020)");
    }

    #[test]
    fn a_locator_goes_inside_the_punctuation() {
        let items = library(&[("AAA", item("Zhang", "2020", "First"))]);

        let mut f = field("f1", "", &["AAA"]);
        f.citation.prefix = Some("see".into());
        f.citation.locator = Some("p. 41".into());
        let author_date = plan(&[f], &items, apa(), Format::Text);
        assert_eq!(author_date.updated[0].text, "(see Zhang, 2020, p. 41)");

        let mut f = field("f1", "", &["AAA"]);
        f.citation.locator = Some("p. 41".into());
        let numeric = plan(&[f], &items, ieee(), Format::Text);
        assert_eq!(numeric.updated[0].text, "[1, p. 41]");
    }

    #[test]
    fn a_citation_of_something_deleted_is_left_alone() {
        // The item is gone from the library; the paper still says what it said,
        // because blanking it destroys a citation the author cannot recover.
        let items = library(&[("AAA", item("Zhang", "2020", "First"))]);
        let fields = vec![field("f1", "(Wu, 1999)", &["GONE"]), field("f2", "", &["AAA"])];

        let plan = plan(&fields, &items, apa(), Format::Text);
        assert_eq!(plan.updated, vec![Rendered { id: "f2".into(), text: "(Zhang, 2020)".into() }]);
        assert_eq!(plan.bibliography.len(), 1, "and it cannot be listed either");
    }

    #[test]
    fn a_missing_item_does_not_reserve_a_number() {
        // Otherwise the paper runs [2], [3] with no [1] in it, and the field
        // text and the bibliography disagree about which number is whose.
        let items = library(&[("AAA", item("Zhang", "2020", "First"))]);
        let fields = vec![field("f1", "", &["GONE"]), field("f2", "", &["AAA"])];

        let plan = plan(&fields, &items, ieee(), Format::Text);
        assert_eq!(text_of(&plan, "f2"), "[1]");
        assert!(plan.bibliography[0].text.starts_with("[1] "));
    }

    #[test]
    fn html_keeps_the_markup_a_word_processor_needs() {
        let items = library(&[("AAA", item("Zhang", "2020", "First"))]);
        let fields = vec![field("f1", "", &["AAA"])];
        let plan = plan(&fields, &items, apa(), Format::Html);
        assert!(
            plan.bibliography[0].text.contains('<'),
            "a journal name is italic, and the add-in inserts HTML"
        );
    }
}
