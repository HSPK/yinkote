//! Pure translation from upstream payloads into [`ItemDraft`].
//!
//! Kept separate from the HTTP clients so every mapping is unit-testable
//! against a recorded fixture — the shapes these APIs return are the part most
//! likely to surprise us, and the part least suited to a live test.

use serde_json::Value;
use yk_core::model::{Creator, ItemDraft};

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn first_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn set(draft: &mut ItemDraft, field: &str, value: Option<String>) {
    if let Some(v) = value {
        draft.fields.insert(field.to_string(), v.into());
    }
}

/// Strip the HTML/JATS markup that abstracts routinely carry.
fn plain_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;
    for c in input.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Crossref
// ---------------------------------------------------------------------------

const CROSSREF_TYPES: &[(&str, &str)] = &[
    ("journal-article", "journalArticle"),
    ("proceedings-article", "conferencePaper"),
    ("posted-content", "preprint"),
    ("book", "book"),
    ("monograph", "book"),
    ("book-chapter", "bookSection"),
    ("dissertation", "thesis"),
    ("report", "report"),
    ("dataset", "dataset"),
    ("standard", "standard"),
    ("peer-review", "document"),
];

/// Map one Crossref `message` object.
pub fn crossref(work: &Value) -> Option<ItemDraft> {
    let title = first_string(work, "title")?;
    let kind = text(work, "type").unwrap_or_default();
    let item_type = CROSSREF_TYPES
        .iter()
        .find(|(from, _)| *from == kind)
        .map(|(_, to)| *to)
        .unwrap_or("journalArticle");

    let mut draft = ItemDraft::new(item_type).with_field("title", title);
    set(&mut draft, "DOI", text(work, "DOI"));
    set(&mut draft, "url", text(work, "URL"));
    set(&mut draft, "publisher", text(work, "publisher"));
    set(&mut draft, "volume", text(work, "volume"));
    set(&mut draft, "issue", text(work, "issue"));
    set(&mut draft, "pages", text(work, "page"));
    set(&mut draft, "language", text(work, "language"));
    set(&mut draft, "ISSN", first_string(work, "ISSN"));
    set(&mut draft, "ISBN", first_string(work, "ISBN"));
    set(&mut draft, "date", issued_date(work));
    set(&mut draft, "abstractNote", text(work, "abstract").map(|a| plain_text(&a)));

    let container = first_string(work, "container-title");
    match item_type {
        "conferencePaper" => set(&mut draft, "proceedingsTitle", container),
        "bookSection" => set(&mut draft, "bookTitle", container),
        _ => set(&mut draft, "publicationTitle", container),
    }

    draft.creators = work
        .get("author")
        .and_then(Value::as_array)
        .map(|list| list.iter().map(crossref_creator).collect())
        .unwrap_or_default();

    draft.tags = work
        .get("subject")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .take(6)
                .map(yk_core::model::ItemTag::automatic)
                .collect()
        })
        .unwrap_or_default();

    Some(draft)
}

fn crossref_creator(author: &Value) -> Creator {
    match (text(author, "family"), text(author, "given")) {
        (Some(family), given) => Creator {
            creator_type: "author".into(),
            first_name: given,
            last_name: Some(family),
            name: None,
        },
        (None, _) => Creator {
            creator_type: "author".into(),
            name: text(author, "name").or_else(|| text(author, "given")),
            ..Default::default()
        },
    }
}

/// `{"issued": {"date-parts": [[2015, 5, 27]]}}` -> `2015-05-27`
fn issued_date(work: &Value) -> Option<String> {
    let parts = work
        .get("issued")
        .or_else(|| work.get("published"))
        .and_then(|v| v.get("date-parts"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_array)?;
    let mut out = String::new();
    for (i, part) in parts.iter().take(3).enumerate() {
        let n = part.as_i64()?;
        if i == 0 {
            out.push_str(&n.to_string());
        } else {
            out.push_str(&format!("-{n:02}"));
        }
    }
    (!out.is_empty()).then_some(out)
}

// ---------------------------------------------------------------------------
// arXiv (Atom)
// ---------------------------------------------------------------------------

/// Read the text of the first `<tag>` inside `xml`, starting at `from`.
fn xml_text(xml: &str, tag: &str, from: usize) -> Option<(String, usize)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = xml[from..].find(&open)? + from;
    let content_start = start + xml[start..].find('>')? + 1;
    let end = xml[content_start..].find(&close)? + content_start;
    let text = crate::meta::decode_entities_public(xml[content_start..end].trim());
    Some((text.split_whitespace().collect::<Vec<_>>().join(" "), end + close.len()))
}

fn xml_all(xml: &str, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((value, next)) = xml_text(xml, tag, at) {
        out.push(value);
        at = next;
    }
    out
}

/// Map one `<entry>` from the arXiv Atom API.
pub fn arxiv(entry: &str) -> Option<ItemDraft> {
    let (title, _) = xml_text(entry, "title", 0)?;
    if title.is_empty() {
        return None;
    }

    let mut draft = ItemDraft::new("preprint").with_field("title", title);
    draft.fields.insert("repository".into(), "arXiv".into());

    if let Some((published, _)) = xml_text(entry, "published", 0) {
        // 2017-06-12T17:57:34Z -> 2017-06-12
        draft.fields.insert("date".into(), published.split('T').next().unwrap_or(&published).into());
    }
    if let Some((summary, _)) = xml_text(entry, "summary", 0) {
        draft.fields.insert("abstractNote".into(), summary.into());
    }
    if let Some((id, _)) = xml_text(entry, "id", 0) {
        draft.fields.insert("url".into(), id.clone().into());
        if let Some(short) = id.rsplit('/').next() {
            draft.fields.insert("arXiv".into(), short.into());
            draft.fields.insert("archiveID".into(), format!("arXiv:{short}").into());
        }
    }
    if let Some((doi, _)) = xml_text(entry, "arxiv:doi", 0) {
        draft.fields.insert("DOI".into(), doi.into());
    }
    if let Some((journal, _)) = xml_text(entry, "arxiv:journal_ref", 0) {
        draft.fields.insert("publicationTitle".into(), journal.into());
    }

    draft.creators = xml_all(entry, "name").iter().map(|n| crate::meta::parse_creator(n)).collect();

    // <category term="cs.LG"/>
    draft.tags = attributes(entry, "category", "term")
        .into_iter()
        .take(5)
        .map(yk_core::model::ItemTag::automatic)
        .collect();

    Some(draft)
}

/// Collect one attribute across every occurrence of a self-closing tag.
fn attributes(xml: &str, tag: &str, attr: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let needle = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rel) = xml[at..].find(&open) {
        let start = at + rel;
        let Some(len) = xml[start..].find('>') else { break };
        let element = &xml[start..start + len];
        if let Some(pos) = element.find(&needle) {
            let value_start = pos + needle.len();
            if let Some(end) = element[value_start..].find('"') {
                out.push(element[value_start..value_start + end].to_string());
            }
        }
        at = start + len + 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Open Library (ISBN)
// ---------------------------------------------------------------------------

pub fn open_library(book: &Value, isbn: &str) -> Option<ItemDraft> {
    let title = text(book, "title")?;
    let mut draft = ItemDraft::new("book").with_field("title", title).with_field("ISBN", isbn);

    set(&mut draft, "date", text(book, "publish_date"));
    set(&mut draft, "url", text(book, "url"));
    set(&mut draft, "numPages", book.get("number_of_pages").map(|v| v.to_string()));
    if let Some(publisher) = book
        .get("publishers")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|p| text(p, "name"))
    {
        draft.fields.insert("publisher".into(), publisher.into());
    }
    if let Some(place) = book
        .get("publish_places")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|p| text(p, "name"))
    {
        draft.fields.insert("place".into(), place.into());
    }

    draft.creators = book
        .get("authors")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().filter_map(|a| text(a, "name")).map(|n| crate::meta::parse_creator(&n)).collect()
        })
        .unwrap_or_default();

    draft.tags = book
        .get("subjects")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| text(s, "name"))
                .take(6)
                .map(yk_core::model::ItemTag::automatic)
                .collect()
        })
        .unwrap_or_default();

    Some(draft)
}

// ---------------------------------------------------------------------------
// PubMed (esummary)
// ---------------------------------------------------------------------------

pub fn pubmed(record: &Value, pmid: &str) -> Option<ItemDraft> {
    let title = text(record, "title")?;
    let mut draft = ItemDraft::new("journalArticle")
        .with_field("title", plain_text(&title))
        .with_field("PMID", pmid);

    set(&mut draft, "publicationTitle", text(record, "fulljournalname").or(text(record, "source")));
    set(&mut draft, "date", text(record, "pubdate"));
    set(&mut draft, "volume", text(record, "volume"));
    set(&mut draft, "issue", text(record, "issue"));
    set(&mut draft, "pages", text(record, "pages"));
    set(&mut draft, "ISSN", text(record, "issn"));

    if let Some(doi) = record
        .get("articleids")
        .and_then(Value::as_array)
        .and_then(|ids| {
            ids.iter().find(|id| text(id, "idtype").as_deref() == Some("doi"))
        })
        .and_then(|id| text(id, "value"))
    {
        draft.fields.insert("DOI".into(), doi.into());
    }

    draft.creators = record
        .get("authors")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().filter_map(|a| text(a, "name")).map(|n| crate::meta::parse_creator(&n)).collect()
        })
        .unwrap_or_default();

    Some(draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_a_crossref_journal_article() {
        let work = json!({
            "type": "journal-article",
            "title": ["Deep learning"],
            "container-title": ["Nature"],
            "DOI": "10.1038/nature14539",
            "volume": "521", "issue": "7553", "page": "436-444",
            "publisher": "Springer Science and Business Media LLC",
            "ISSN": ["0028-0836", "1476-4687"],
            "issued": { "date-parts": [[2015, 5, 27]] },
            "abstract": "<jats:p>Deep learning allows <jats:italic>models</jats:italic>.</jats:p>",
            "author": [
                { "given": "Yann", "family": "LeCun" },
                { "given": "Yoshua", "family": "Bengio" }
            ],
            "subject": ["Multidisciplinary"]
        });
        let d = crossref(&work).unwrap();
        assert_eq!(d.item_type, "journalArticle");
        assert_eq!(d.fields["title"], "Deep learning");
        assert_eq!(d.fields["publicationTitle"], "Nature");
        assert_eq!(d.fields["date"], "2015-05-27");
        assert_eq!(d.fields["ISSN"], "0028-0836");
        assert_eq!(d.fields["abstractNote"], "Deep learning allows models.");
        assert_eq!(d.creators.len(), 2);
        assert_eq!(d.creators[0].last_name.as_deref(), Some("LeCun"));
        assert_eq!(d.tags[0].tag, "Multidisciplinary");
        assert_eq!(d.tags[0].r#type, 1, "imported tags are automatic");
    }

    #[test]
    fn maps_crossref_types_to_the_right_container_field() {
        let paper = json!({
            "type": "proceedings-article", "title": ["P"], "container-title": ["NeurIPS"]
        });
        assert_eq!(crossref(&paper).unwrap().fields["proceedingsTitle"], "NeurIPS");

        let chapter = json!({ "type": "book-chapter", "title": ["C"], "container-title": ["A Book"] });
        assert_eq!(crossref(&chapter).unwrap().fields["bookTitle"], "A Book");
    }

    #[test]
    fn crossref_handles_partial_dates_and_institutional_authors() {
        let work = json!({
            "type": "report", "title": ["R"],
            "issued": { "date-parts": [[2020]] },
            "author": [{ "name": "World Health Organization" }]
        });
        let d = crossref(&work).unwrap();
        assert_eq!(d.fields["date"], "2020");
        assert_eq!(d.creators[0].name.as_deref(), Some("World Health Organization"));
    }

    #[test]
    fn crossref_without_a_title_is_rejected() {
        assert!(crossref(&json!({ "type": "journal-article" })).is_none());
    }

    const ARXIV_ENTRY: &str = r#"
    <entry>
      <id>http://arxiv.org/abs/1706.03762v5</id>
      <published>2017-06-12T17:57:34Z</published>
      <title>Attention Is All You
       Need</title>
      <summary>  The dominant sequence transduction models are based on
        complex recurrent networks.  </summary>
      <author><name>Ashish Vaswani</name></author>
      <author><name>Noam Shazeer</name></author>
      <arxiv:doi>10.48550/arXiv.1706.03762</arxiv:doi>
      <category term="cs.CL" scheme="http://arxiv.org/schemas/atom"/>
      <category term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
    </entry>"#;

    #[test]
    fn maps_an_arxiv_entry() {
        let d = arxiv(ARXIV_ENTRY).unwrap();
        assert_eq!(d.item_type, "preprint");
        assert_eq!(d.fields["title"], "Attention Is All You Need", "line wrapping is normalised");
        assert_eq!(d.fields["date"], "2017-06-12");
        assert_eq!(d.fields["arXiv"], "1706.03762v5");
        assert_eq!(d.fields["DOI"], "10.48550/arXiv.1706.03762");
        assert!(d.fields["abstractNote"].as_str().unwrap().starts_with("The dominant"));
        assert_eq!(d.creators.len(), 2);
        assert_eq!(d.creators[0].last_name.as_deref(), Some("Vaswani"));
        assert_eq!(d.tags.len(), 2);
        assert_eq!(d.tags[0].tag, "cs.CL");
    }

    #[test]
    fn arxiv_without_a_title_is_rejected() {
        assert!(arxiv("<entry><id>x</id></entry>").is_none());
    }

    #[test]
    fn maps_an_open_library_record() {
        let book = json!({
            "title": "Designing Data-Intensive Applications",
            "publish_date": "2017",
            "number_of_pages": 616,
            "publishers": [{ "name": "O'Reilly" }],
            "publish_places": [{ "name": "Sebastopol" }],
            "authors": [{ "name": "Martin Kleppmann" }],
            "subjects": [{ "name": "Databases" }]
        });
        let d = open_library(&book, "9781449373320").unwrap();
        assert_eq!(d.item_type, "book");
        assert_eq!(d.fields["ISBN"], "9781449373320");
        assert_eq!(d.fields["publisher"], "O'Reilly");
        assert_eq!(d.fields["numPages"], "616");
        assert_eq!(d.creators[0].last_name.as_deref(), Some("Kleppmann"));
    }

    #[test]
    fn maps_a_pubmed_summary() {
        let record = json!({
            "title": "Deep learning in <b>medicine</b>.",
            "fulljournalname": "Nature Medicine",
            "pubdate": "2019 Jan",
            "volume": "25", "issue": "1", "pages": "24-29",
            "authors": [{ "name": "Topol EJ" }],
            "articleids": [
                { "idtype": "pubmed", "value": "30617335" },
                { "idtype": "doi", "value": "10.1038/s41591-018-0300-7" }
            ]
        });
        let d = pubmed(&record, "30617335").unwrap();
        assert_eq!(d.fields["title"], "Deep learning in medicine.");
        assert_eq!(d.fields["PMID"], "30617335");
        assert_eq!(d.fields["DOI"], "10.1038/s41591-018-0300-7");
        assert_eq!(d.fields["publicationTitle"], "Nature Medicine");
    }

    #[test]
    fn strips_markup_but_keeps_words() {
        assert_eq!(plain_text("<p>a <i>b</i> c</p>"), "a b c");
        assert_eq!(plain_text("no markup"), "no markup");
    }
}
