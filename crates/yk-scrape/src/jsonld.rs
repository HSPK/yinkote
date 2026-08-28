//! Reading a page's schema.org description.
//!
//! **Why this is worth having.** The `citation_*` meta tags are a publishing
//! convention: academic publishers emit them, and almost nobody else does. A
//! preprint on a lab's own site, a chapter on a university press page, a post
//! on a research blog, a news article somebody wants to cite — these carry
//! their metadata as JSON-LD instead, because that is what search engines read.
//! Without it those pages save as a bare title and a URL.
//!
//! **How it fits.** The values are folded into the same [`PageMeta`] map the
//! meta tags land in, under `ld:` keys, and the field lists in
//! [`super::to_draft`] name those keys *last*. So JSON-LD fills gaps and never
//! overrides a publisher's own tags — the precedence is visible in the lists
//! rather than hidden in an "insert if absent".
//!
//! **What is deliberately not done.** No `@context` fetching, no remote schema
//! resolution, and no attempt at the full specification. A page either says
//! `"headline": "…"` in a way anybody can read or it does not; chasing the rest
//! of JSON-LD would be a great deal of work for pages that are already covered.

use serde_json::Value;

/// Types worth treating as something citeable.
///
/// A `WebSite` or `Organization` block describes the *publisher*, not the page,
/// and reading a title out of one gives every article on the site the same
/// name. Only types that denote a work are considered.
const WORKS: [&str; 9] = [
    "Article",
    "NewsArticle",
    "BlogPosting",
    "ScholarlyArticle",
    "Report",
    "Book",
    "Chapter",
    "Thesis",
    "TechArticle",
];

/// Every `<script type="application/ld+json">` payload in the document.
fn blocks(html: &str) -> Vec<Value> {
    let lower = html.to_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = lower[i..].find("<script") {
        let start = i + rel;
        let Some(open_len) = lower[start..].find('>') else { break };
        let open = &lower[start..start + open_len];
        let body_from = start + open_len + 1;
        let Some(end_rel) = lower[body_from..].find("</script") else { break };
        if open.contains("application/ld+json") {
            if let Ok(value) = serde_json::from_str::<Value>(&html[body_from..body_from + end_rel]) {
                out.push(value);
            }
        }
        i = body_from + end_rel;
    }
    out
}

/// Flatten the shapes a page may wrap its description in.
///
/// A block is an object, or an array of them, or an object with a `@graph`
/// holding the array. All three are common, and a reader that handles only the
/// first finds nothing on a great many sites.
fn objects(value: &Value, into: &mut Vec<Value>) {
    match value {
        Value::Array(items) => items.iter().for_each(|v| objects(v, into)),
        Value::Object(map) => {
            if let Some(graph) = map.get("@graph") {
                objects(graph, into);
            }
            into.push(value.clone());
        }
        _ => {}
    }
}

/// Whether this object describes a work rather than its surroundings.
fn is_work(node: &Value) -> bool {
    let kind = node.get("@type");
    let names: Vec<&str> = match kind {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => return false,
    };
    names.iter().any(|n| WORKS.contains(n))
}

/// A string from a field that may be a string, a list, or an object with a
/// `name` — all three of which appear in the wild for the same field.
fn text(node: &Value, key: &str) -> Option<String> {
    fn one(value: &Value) -> Option<String> {
        match value {
            Value::String(s) => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Object(map) => map.get("name").and_then(one),
            Value::Array(items) => items.iter().find_map(one),
            _ => None,
        }
    }
    one(node.get(key)?).filter(|s| !s.is_empty())
}

/// Every author name, in order.
fn authors(node: &Value) -> Vec<String> {
    fn names(value: &Value, into: &mut Vec<String>) {
        match value {
            Value::String(s) => into.push(s.trim().to_string()),
            Value::Object(map) => {
                if let Some(Value::String(name)) = map.get("name") {
                    into.push(name.trim().to_string());
                }
            }
            Value::Array(items) => items.iter().for_each(|v| names(v, into)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(value) = node.get("author").or_else(|| node.get("creator")) {
        names(value, &mut out);
    }
    out.retain(|n| !n.is_empty());
    out
}

/// The `ld:` entries a document's JSON-LD contributes.
///
/// Returns pairs rather than mutating, so the extraction is testable without a
/// `PageMeta` and the caller decides what to do with them.
pub fn fields(html: &str) -> Vec<(String, String)> {
    let mut nodes = Vec::new();
    for block in blocks(html) {
        objects(&block, &mut nodes);
    }
    let Some(work) = nodes.iter().find(|n| is_work(n)) else { return Vec::new() };

    let mut out = Vec::new();
    let mut put = |key: &str, value: Option<String>| {
        if let Some(v) = value {
            out.push((format!("ld:{key}"), v));
        }
    };
    // `headline` is what news and blog schemas use; `name` is what everything
    // else does, and plenty of pages set both to the same string.
    put("title", text(work, "headline").or_else(|| text(work, "name")));
    put("abstract", text(work, "description").or_else(|| text(work, "abstract")));
    put("date", text(work, "datePublished").or_else(|| text(work, "dateCreated")));
    put("publisher", text(work, "publisher"));
    put("publication", text(work, "isPartOf"));
    put("language", text(work, "inLanguage"));
    put("isbn", text(work, "isbn"));
    put("issn", text(work, "issn"));
    put("type", text(work, "@type"));

    // A DOI arrives as an identifier, sometimes prefixed as a URL.
    if let Some(doi) = text(work, "doi").or_else(|| text(work, "identifier")) {
        let bare = doi.trim_start_matches("https://doi.org/").trim_start_matches("doi:");
        if bare.starts_with("10.") {
            out.push(("ld:doi".into(), bare.to_string()));
        }
    }

    for name in authors(work) {
        out.push(("ld:author".into(), name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEWS: &str = r#"<html><head>
      <script type="application/ld+json">
      {
        "@context": "https://schema.org",
        "@type": "NewsArticle",
        "headline": "A discovery",
        "description": "What was found.",
        "datePublished": "2024-03-04T10:00:00Z",
        "inLanguage": "en",
        "author": [{"@type": "Person", "name": "Ada Lovelace"},
                   {"@type": "Person", "name": "Charles Babbage"}],
        "publisher": {"@type": "Organization", "name": "The Paper"}
      }
      </script></head><body></body></html>"#;

    fn map(html: &str) -> std::collections::HashMap<String, Vec<String>> {
        let mut out: std::collections::HashMap<String, Vec<String>> = Default::default();
        for (k, v) in fields(html) {
            out.entry(k).or_default().push(v);
        }
        out
    }

    #[test]
    fn reads_a_news_article() {
        let m = map(NEWS);
        assert_eq!(m["ld:title"], ["A discovery"]);
        assert_eq!(m["ld:abstract"], ["What was found."]);
        assert_eq!(m["ld:date"], ["2024-03-04T10:00:00Z"]);
        assert_eq!(m["ld:publisher"], ["The Paper"], "an object with a name, not a string");
        assert_eq!(m["ld:author"], ["Ada Lovelace", "Charles Babbage"]);
    }

    #[test]
    fn finds_the_work_inside_a_graph() {
        // Sites built with common CMS plugins wrap everything in @graph, with
        // the WebSite and Organization first. A reader that only understands a
        // bare object finds nothing on any of them.
        let html = r#"<script type="application/ld+json">
        {"@context":"https://schema.org","@graph":[
          {"@type":"WebSite","name":"Some Journal"},
          {"@type":"Organization","name":"Some Publisher"},
          {"@type":"ScholarlyArticle","headline":"The actual paper"}
        ]}</script>"#;
        assert_eq!(map(html)["ld:title"], ["The actual paper"]);
    }

    #[test]
    fn ignores_blocks_that_describe_the_site_rather_than_the_page() {
        // Reading a title out of a WebSite block gives every article on the
        // site the same name.
        let html = r#"<script type="application/ld+json">
        {"@type":"WebSite","name":"Some Journal","url":"https://x"}</script>"#;
        assert!(fields(html).is_empty());
    }

    #[test]
    fn accepts_a_type_given_as_a_list() {
        let html = r#"<script type="application/ld+json">
        {"@type":["CreativeWork","ScholarlyArticle"],"name":"Both at once"}</script>"#;
        assert_eq!(map(html)["ld:title"], ["Both at once"]);
    }

    #[test]
    fn takes_a_doi_however_it_is_written() {
        for raw in ["10.1038/nature14539", "https://doi.org/10.1038/nature14539", "doi:10.1038/nature14539"] {
            let html = format!(
                r#"<script type="application/ld+json">{{"@type":"ScholarlyArticle","name":"x","identifier":"{raw}"}}</script>"#
            );
            assert_eq!(map(&html)["ld:doi"], ["10.1038/nature14539"], "{raw}");
        }
    }

    #[test]
    fn an_identifier_that_is_not_a_doi_is_left_alone() {
        // Plenty of pages put an internal article number here.
        let html = r#"<script type="application/ld+json">
        {"@type":"Article","name":"x","identifier":"ART-99213"}</script>"#;
        assert!(!map(html).contains_key("ld:doi"));
    }

    #[test]
    fn malformed_json_is_skipped_rather_than_fatal() {
        // One broken block on a page must not lose the good one beside it.
        let html = r#"
          <script type="application/ld+json">{ not json </script>
          <script type="application/ld+json">{"@type":"Article","name":"Still here"}</script>"#;
        assert_eq!(map(html)["ld:title"], ["Still here"]);
    }

    #[test]
    fn other_scripts_are_not_mistaken_for_it() {
        let html = r#"<script>var x = {"@type":"Article","name":"not metadata"}</script>"#;
        assert!(fields(html).is_empty());
    }

    #[test]
    fn a_page_with_none_contributes_nothing() {
        assert!(fields("<html><head><title>Plain</title></head></html>").is_empty());
    }
}
