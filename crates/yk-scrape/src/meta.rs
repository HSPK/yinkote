//! Metadata embedded in web pages.
//!
//! Almost every publisher, repository and preprint server emits Highwire
//! (`citation_*`), Dublin Core or Open Graph meta tags. Reading those is far
//! more reliable than scraping the rendered page, and it works for the long
//! tail of sites nobody has written a dedicated adapter for.
//!
//! A focused scanner beats pulling in a full HTML parser here: we only need
//! `<meta>` and `<title>`, and this way the crate stays dependency-light and
//! completely testable offline.

use std::collections::HashMap;

use yk_core::model::{Creator, ItemDraft};

/// Meta tags keyed by a lower-cased `name`/`property`, preserving repeats
/// (authors appear once per person).
#[derive(Debug, Default)]
pub struct PageMeta {
    pub title: Option<String>,
    tags: HashMap<String, Vec<String>>,
}

impl PageMeta {
    pub fn first(&self, key: &str) -> Option<&str> {
        self.tags.get(key).and_then(|v| v.first()).map(String::as_str)
    }

    pub fn all(&self, key: &str) -> &[String] {
        self.tags.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// First value present among `keys`, in preference order.
    fn any(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().find_map(|k| self.first(k))
    }

    fn any_all(&self, keys: &[&str]) -> &[String] {
        keys.iter().map(|k| self.all(k)).find(|v| !v.is_empty()).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.title.is_none()
    }
}

/// Extract `<title>` and every `<meta>` tag from an HTML document.
pub fn parse(html: &str) -> PageMeta {
    let mut meta = PageMeta::default();
    let lower = html.to_lowercase();

    if let Some(start) = lower.find("<title") {
        if let Some(open) = lower[start..].find('>') {
            let from = start + open + 1;
            if let Some(end) = lower[from..].find("</title>") {
                let text = decode_entities(html[from..from + end].trim());
                if !text.is_empty() {
                    meta.title = Some(text);
                }
            }
        }
    }

    let mut i = 0;
    while let Some(rel) = lower[i..].find("<meta") {
        let start = i + rel;
        let Some(len) = lower[start..].find('>') else { break };
        let tag = &html[start..start + len];
        if let Some((key, value)) = parse_meta_tag(tag) {
            meta.tags.entry(key).or_default().push(value);
        }
        i = start + len + 1;
    }

    // schema.org, under `ld:` keys. The field lists in `to_draft` name those
    // last, so a page's JSON-LD fills gaps and never overrides a publisher's
    // own `citation_*` tags. See `jsonld` for why it is worth reading at all.
    for (key, value) in crate::jsonld::fields(html) {
        meta.tags.entry(key).or_default().push(value);
    }
    meta
}

fn parse_meta_tag(tag: &str) -> Option<(String, String)> {
    let name = attribute(tag, "name")
        .or_else(|| attribute(tag, "property"))
        .or_else(|| attribute(tag, "itemprop"))?;
    let content = attribute(tag, "content")?;
    let name = name.trim().to_lowercase();
    let content = content.trim().to_string();
    (!name.is_empty() && !content.is_empty()).then_some((name, content))
}

/// Read `attr="value"`, `attr='value'` or `attr=value` from a tag.
fn attribute(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(attr) {
        let at = from + rel;
        // Must be preceded by whitespace so `name` does not match `itemname`.
        let boundary = at == 0 || lower[..at].ends_with(|c: char| c.is_whitespace() || c == '<');
        let after = &lower[at + attr.len()..];
        let trimmed = after.trim_start();
        if boundary && trimmed.starts_with('=') {
            let value_start = at + attr.len() + (after.len() - trimmed.len()) + 1;
            let rest = tag[value_start..].trim_start();
            let offset = value_start + (tag[value_start..].len() - rest.len());
            let value = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let end = tag[offset + 1..].find(q)?;
                    &tag[offset + 1..offset + 1 + end]
                }
                _ => {
                    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
                    &rest[..end]
                }
            };
            return Some(decode_entities(value));
        }
        from = at + attr.len();
    }
    None
}

/// Entity decoding, also used by the XML mappings.
pub fn decode_entities_public(input: &str) -> String {
    decode_entities(input)
}

/// The handful of entities that actually appear in meta content.
fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find(';').filter(|e| *e <= 10);
        match end {
            Some(end) => {
                let entity = &rest[1..end];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" | "#x27" => Some('\''),
                    "nbsp" => Some(' '),
                    _ => entity
                        .strip_prefix("#x")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                };
                match decoded {
                    Some(c) => {
                        out.push(c);
                        rest = &rest[end + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Split "Family, Given" or "Given Family" into a creator.
pub fn parse_creator(raw: &str) -> Creator {
    let raw = raw.trim();
    if let Some((family, given)) = raw.split_once(',') {
        return Creator {
            creator_type: "author".into(),
            first_name: Some(given.trim().to_string()).filter(|s| !s.is_empty()),
            last_name: Some(family.trim().to_string()),
            name: None,
        };
    }
    // CJK names are not space-separated; keep them in one field.
    if raw.chars().any(yk_core::text::is_cjk) || !raw.contains(' ') {
        return Creator { creator_type: "author".into(), name: Some(raw.to_string()), ..Default::default() };
    }
    let (given, family) = raw.rsplit_once(' ').unwrap_or(("", raw));
    Creator {
        creator_type: "author".into(),
        first_name: Some(given.trim().to_string()).filter(|s| !s.is_empty()),
        last_name: Some(family.trim().to_string()),
        name: None,
    }
}

/// Decide the Yinkote item type from whatever the page advertises.
fn item_type(meta: &PageMeta) -> &'static str {
    if meta.first("citation_journal_title").is_some() || meta.first("citation_issn").is_some() {
        return "journalArticle";
    }
    if meta.first("citation_conference_title").is_some() {
        return "conferencePaper";
    }
    if meta.first("citation_dissertation_institution").is_some() {
        return "thesis";
    }
    if meta.first("citation_technical_report_institution").is_some() {
        return "report";
    }
    if meta.first("citation_isbn").is_some() || meta.any(&["og:type"]) == Some("book") {
        return "book";
    }
    if meta.first("citation_title").is_some() {
        // Highwire without a venue is typically a preprint server.
        return "preprint";
    }
    "webpage"
}

/// Build a draft from page metadata. Returns `None` when the page carries
/// nothing worth saving beyond a bare URL.
pub fn to_draft(meta: &PageMeta, url: &str) -> Option<ItemDraft> {
    let title = meta
        .any(&["citation_title", "dc.title", "og:title", "twitter:title", "ld:title"])
        .map(str::to_string)
        .or_else(|| meta.title.clone())?;
    if title.trim().is_empty() {
        return None;
    }

    let mut draft = ItemDraft::new(item_type(meta))
        .with_field("title", title.trim())
        .with_field("url", url);

    let mut set = |field: &str, keys: &[&str]| {
        if let Some(v) = meta.any(keys) {
            draft.fields.insert(field.to_string(), v.into());
        }
    };
    set("abstractNote", &["citation_abstract", "dc.description", "og:description", "description", "ld:abstract"]);
    set("date", &["citation_publication_date", "citation_date", "dc.date", "article:published_time", "ld:date"]);
    set("publicationTitle", &["citation_journal_title", "og:site_name", "ld:publication"]);
    set("bookTitle", &["citation_book_title"]);
    set("proceedingsTitle", &["citation_conference_title"]);
    set("volume", &["citation_volume"]);
    set("issue", &["citation_issue"]);
    set("publisher", &["citation_publisher", "dc.publisher", "ld:publisher"]);
    set("language", &["citation_language", "dc.language", "og:locale", "ld:language"]);
    set("DOI", &["citation_doi", "dc.identifier.doi", "ld:doi"]);
    set("ISSN", &["citation_issn", "ld:issn"]);
    set("ISBN", &["citation_isbn", "ld:isbn"]);
    set("university", &["citation_dissertation_institution"]);
    set("institution", &["citation_technical_report_institution"]);
    set("websiteTitle", &["og:site_name"]);

    if let (Some(first), Some(last)) =
        (meta.first("citation_firstpage"), meta.first("citation_lastpage"))
    {
        draft.fields.insert("pages".into(), format!("{first}-{last}").into());
    } else if let Some(first) = meta.first("citation_firstpage") {
        draft.fields.insert("pages".into(), first.into());
    }

    draft.creators = meta
        .any_all(&["citation_author", "dc.creator", "author", "article:author", "ld:author"])
        .iter()
        .flat_map(|raw| raw.split(';'))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_creator)
        .collect();

    Some(draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGHWIRE: &str = r#"
    <html><head>
      <title>Deep learning | Nature</title>
      <meta name="citation_title" content="Deep learning">
      <meta name="citation_author" content="LeCun, Yann">
      <meta name="citation_author" content="Bengio, Yoshua">
      <meta name="citation_author" content="Hinton, Geoffrey">
      <meta name="citation_journal_title" content="Nature">
      <meta name="citation_publication_date" content="2015/05/27">
      <meta name="citation_volume" content="521">
      <meta name="citation_issue" content="7553">
      <meta name="citation_firstpage" content="436">
      <meta name="citation_lastpage" content="444">
      <meta name="citation_doi" content="10.1038/nature14539">
      <meta name="citation_issn" content="0028-0836">
      <meta name="citation_abstract" content="Deep learning allows models &amp; more">
    </head></html>"#;

    #[test]
    fn extracts_highwire_tags() {
        let meta = parse(HIGHWIRE);
        assert_eq!(meta.first("citation_title"), Some("Deep learning"));
        assert_eq!(meta.all("citation_author").len(), 3);
        assert_eq!(meta.title.as_deref(), Some("Deep learning | Nature"));
    }

    #[test]
    fn builds_a_journal_article_draft() {
        let draft = to_draft(&parse(HIGHWIRE), "https://nature.com/x").unwrap();
        assert_eq!(draft.item_type, "journalArticle");
        assert_eq!(draft.fields["title"], "Deep learning");
        assert_eq!(draft.fields["publicationTitle"], "Nature");
        assert_eq!(draft.fields["pages"], "436-444");
        assert_eq!(draft.fields["DOI"], "10.1038/nature14539");
        assert_eq!(draft.creators.len(), 3);
        assert_eq!(draft.creators[0].last_name.as_deref(), Some("LeCun"));
        assert_eq!(draft.creators[0].first_name.as_deref(), Some("Yann"));
    }

    #[test]
    fn decodes_entities_in_content() {
        let draft = to_draft(&parse(HIGHWIRE), "u").unwrap();
        assert_eq!(draft.fields["abstractNote"], "Deep learning allows models & more");
    }

    #[test]
    fn falls_back_to_open_graph() {
        let html = r#"<meta property="og:title" content="A Blog Post">
                      <meta property="og:description" content="Some words">
                      <meta property="og:site_name" content="Example">"#;
        let draft = to_draft(&parse(html), "https://example.com/p").unwrap();
        assert_eq!(draft.item_type, "webpage");
        assert_eq!(draft.fields["title"], "A Blog Post");
        assert_eq!(draft.fields["abstractNote"], "Some words");
        assert_eq!(draft.fields["websiteTitle"], "Example");
    }

    #[test]
    fn falls_back_to_the_document_title() {
        let draft = to_draft(&parse("<html><head><title>Only a title</title></head>"), "u").unwrap();
        assert_eq!(draft.fields["title"], "Only a title");
    }

    #[test]
    fn returns_nothing_without_a_title() {
        assert!(to_draft(&parse("<html><body>text</body></html>"), "u").is_none());
    }

    #[test]
    fn handles_single_quotes_and_bare_attributes() {
        let meta = parse("<meta name='citation_title' content='Quoted'><meta name=x content=y>");
        assert_eq!(meta.first("citation_title"), Some("Quoted"));
        assert_eq!(meta.first("x"), Some("y"));
    }

    #[test]
    fn ignores_attribute_names_that_merely_contain_name() {
        let meta = parse(r#"<meta itemname="nope" name="real" content="v">"#);
        assert_eq!(meta.first("real"), Some("v"));
    }

    #[test]
    fn detects_specialised_item_types() {
        let thesis = parse(r#"<meta name="citation_title" content="T">
                              <meta name="citation_dissertation_institution" content="MIT">"#);
        assert_eq!(to_draft(&thesis, "u").unwrap().item_type, "thesis");

        let book = parse(r#"<meta name="citation_title" content="B">
                            <meta name="citation_isbn" content="9787111213826">"#);
        assert_eq!(to_draft(&book, "u").unwrap().item_type, "book");

        let preprint = parse(r#"<meta name="citation_title" content="P">"#);
        assert_eq!(to_draft(&preprint, "u").unwrap().item_type, "preprint");
    }

    #[test]
    fn splits_creator_names_correctly() {
        assert_eq!(parse_creator("LeCun, Yann").last_name.as_deref(), Some("LeCun"));
        assert_eq!(parse_creator("Yann LeCun").last_name.as_deref(), Some("LeCun"));
        assert_eq!(parse_creator("Yann LeCun").first_name.as_deref(), Some("Yann"));
        // CJK and mononyms stay in the single-field form.
        assert_eq!(parse_creator("张伟").name.as_deref(), Some("张伟"));
        assert_eq!(parse_creator("Plato").name.as_deref(), Some("Plato"));
    }

    #[test]
    fn splits_semicolon_separated_authors() {
        let meta = parse(
            r#"<meta name="citation_title" content="T">
               <meta name="dc.creator" content="Smith, A; Jones, B">"#,
        );
        assert_eq!(to_draft(&meta, "u").unwrap().creators.len(), 2);
    }

    #[test]
    fn empty_document_is_empty() {
        assert!(parse("").is_empty());
    }
}

#[cfg(test)]
mod jsonld_precedence_tests {
    use super::*;

    const LD_ONLY: &str = r#"<html><head>
      <title>Some site</title>
      <script type="application/ld+json">
      {"@type":"ScholarlyArticle",
       "headline":"Structure from schema.org",
       "description":"An abstract.",
       "datePublished":"2023-09-01",
       "author":[{"@type":"Person","name":"Zhang, Wei"}],
       "publisher":{"name":"A Press"},
       "identifier":"10.5555/abcd"}
      </script></head></html>"#;

    #[test]
    fn a_page_with_no_citation_tags_is_still_worth_saving() {
        // The whole point: `citation_*` is a publishing convention that academic
        // publishers follow and almost nobody else does. Without this, a
        // preprint on a lab's own site saved as a title and a URL.
        let draft = to_draft(&parse(LD_ONLY), "https://lab.example/paper").unwrap();
        assert_eq!(draft.fields.get("title").unwrap(), "Structure from schema.org");
        assert_eq!(draft.fields.get("abstractNote").unwrap(), "An abstract.");
        assert_eq!(draft.fields.get("date").unwrap(), "2023-09-01");
        assert_eq!(draft.fields.get("DOI").unwrap(), "10.5555/abcd");
        assert_eq!(draft.fields.get("publisher").unwrap(), "A Press");
        assert_eq!(draft.creators.len(), 1);
        assert_eq!(draft.creators[0].last_name.as_deref(), Some("Zhang"));
    }

    #[test]
    fn the_publishers_own_tags_win() {
        // Both are present on plenty of publisher pages, and they disagree:
        // JSON-LD is frequently the site's summary of the page while the
        // Highwire tags are the record of the article. The record wins.
        let both = r#"<html><head>
          <meta name="citation_title" content="The article's real title">
          <meta name="citation_author" content="Lovelace, Ada">
          <script type="application/ld+json">
          {"@type":"Article","headline":"Read this on Our Site!","author":"Our Staff"}
          </script></head></html>"#;
        let draft = to_draft(&parse(both), "https://publisher.example/x").unwrap();
        assert_eq!(draft.fields.get("title").unwrap(), "The article's real title");
        assert_eq!(draft.creators[0].last_name.as_deref(), Some("Lovelace"));
    }

    #[test]
    fn json_ld_fills_only_the_gaps_a_publisher_left() {
        let partial = r#"<html><head>
          <meta name="citation_title" content="Half a record">
          <script type="application/ld+json">
          {"@type":"ScholarlyArticle","name":"Half a record","datePublished":"2020-01-02"}
          </script></head></html>"#;
        let draft = to_draft(&parse(partial), "https://x/y").unwrap();
        assert_eq!(draft.fields.get("title").unwrap(), "Half a record");
        assert_eq!(draft.fields.get("date").unwrap(), "2020-01-02", "the tag was absent");
    }

    #[test]
    fn a_page_with_neither_is_still_no_worse_than_before() {
        let bare = "<html><head><title>Just a page</title></head></html>";
        let draft = to_draft(&parse(bare), "https://x/y").unwrap();
        assert_eq!(draft.fields.get("title").unwrap(), "Just a page");
        assert_eq!(draft.item_type, "webpage");
    }
}
