//! Turning each service's answer into [`Found`].
//!
//! Pure functions over a payload, with no network in sight: the shapes are
//! where the mistakes live, and a test that needs arXiv to be up is a test
//! that goes red for somebody else's outage.

use super::Found;

/// The order an identifier is preferred in.
///
/// A DOI resolves to the version of record; an arXiv id resolves to a
/// preprint; a URL resolves to whatever the page happens to say. Handing back
/// the weakest one available makes every later step worse.
fn best(doi: Option<String>, arxiv: Option<String>, url: Option<&str>) -> Option<String> {
    doi.or(arxiv).or_else(|| url.map(str::to_string))
}

/// arXiv answers in Atom. Parsed by hand because the whole feed is four tags
/// deep and a full XML parser is a dependency for nothing.
pub fn arxiv_feed(xml: &str, limit: usize) -> Vec<Found> {
    let mut out = Vec::new();
    for entry in between_all(xml, "<entry", "</entry>").take(limit) {
        let id = tag(entry, "id");
        // `http://arxiv.org/abs/2401.12345v2` — the number is the identifier
        // and the version is not part of the paper's identity.
        let arxiv = id.as_deref().and_then(|i| i.rsplit("/abs/").next()).map(str::to_string);
        let title = tag(entry, "title").unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        out.push(Found {
            title,
            authors: between_all(entry, "<author", "</author>")
                .filter_map(|a| tag(a, "name"))
                .collect(),
            year: tag(entry, "published")
                .and_then(|d| d.get(..4).and_then(|y| y.parse().ok())),
            venue: Some("arXiv".into()),
            identifier: best(
                attribute_of(entry, "doi"),
                arxiv.clone(),
                id.as_deref(),
            ),
            url: id,
            summary: tag(entry, "summary"),
            source: "arxiv",
        });
    }
    out
}

/// Crossref answers `message.items`.
pub fn crossref_works(body: &serde_json::Value, limit: usize) -> Vec<Found> {
    let Some(items) = body.pointer("/message/items").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .take(limit)
        .filter_map(|work| {
            let title = work
                .pointer("/title/0")
                .and_then(|v| v.as_str())
                .filter(|t| !t.is_empty())?
                .to_string();
            let doi = work.get("DOI").and_then(|v| v.as_str()).map(str::to_string);
            Some(Found {
                title,
                authors: work
                    .get("author")
                    .and_then(|v| v.as_array())
                    .map(|list| list.iter().filter_map(person).collect())
                    .unwrap_or_default(),
                year: work
                    .pointer("/issued/date-parts/0/0")
                    .and_then(serde_json::Value::as_i64),
                venue: work
                    .pointer("/container-title/0")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                url: work.get("URL").and_then(|v| v.as_str()).map(str::to_string),
                identifier: doi,
                summary: work.get("abstract").and_then(|v| v.as_str()).map(strip_tags),
                source: "crossref",
            })
        })
        .collect()
}

/// OpenAlex answers `results`, and inverts its abstracts.
pub fn openalex_works(body: &serde_json::Value, limit: usize) -> Vec<Found> {
    let Some(items) = body.get("results").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .take(limit)
        .filter_map(|work| {
            let title = work
                .get("display_name")
                .and_then(|v| v.as_str())
                .filter(|t| !t.is_empty())?
                .to_string();
            // OpenAlex gives DOIs as full URLs; the bare one is what resolves.
            let doi = work
                .get("doi")
                .and_then(|v| v.as_str())
                .map(|d| d.trim_start_matches("https://doi.org/").to_string());
            Some(Found {
                title,
                authors: work
                    .get("authorships")
                    .and_then(|v| v.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|a| {
                                a.pointer("/author/display_name")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                year: work.get("publication_year").and_then(serde_json::Value::as_i64),
                venue: work
                    .pointer("/primary_location/source/display_name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                identifier: doi,
                url: work
                    .pointer("/primary_location/landing_page_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                summary: work.get("abstract_inverted_index").map(inverted_abstract),
                source: "openalex",
            })
        })
        .collect()
}

/// PubMed's esummary answers a map keyed by id, plus a `uids` list.
pub fn pubmed_summaries(body: &serde_json::Value, limit: usize) -> Vec<Found> {
    let Some(uids) = body.pointer("/result/uids").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    uids.iter()
        .take(limit)
        .filter_map(|uid| {
            let uid = uid.as_str()?;
            let record = body.pointer(&format!("/result/{uid}"))?;
            let title = record
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|t| !t.is_empty())?
                .to_string();
            let doi = record
                .get("articleids")
                .and_then(|v| v.as_array())
                .and_then(|ids| {
                    ids.iter()
                        .find(|i| i.get("idtype").and_then(|v| v.as_str()) == Some("doi"))
                        .and_then(|i| i.get("value").and_then(|v| v.as_str()))
                })
                .map(str::to_string);
            let url = format!("https://pubmed.ncbi.nlm.nih.gov/{uid}/");
            Some(Found {
                title: strip_tags(&title),
                authors: record
                    .get("authors")
                    .and_then(|v| v.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                year: record
                    .get("pubdate")
                    .and_then(|v| v.as_str())
                    .and_then(|d| d.get(..4).and_then(|y| y.parse().ok())),
                venue: record.get("fulljournalname").and_then(|v| v.as_str()).map(str::to_string),
                identifier: best(doi, None, Some(&url)),
                url: Some(url),
                summary: None,
                source: "pubmed",
            })
        })
        .collect()
}

fn person(author: &serde_json::Value) -> Option<String> {
    let given = author.get("given").and_then(|v| v.as_str()).unwrap_or_default();
    let family = author.get("family").and_then(|v| v.as_str()).unwrap_or_default();
    let joined = format!("{given} {family}").trim().to_string();
    (!joined.is_empty()).then_some(joined)
}

/// OpenAlex stores abstracts as a word → positions map, to sidestep the
/// copyright on the abstract itself. Putting the words back in order is the
/// only way to read one.
fn inverted_abstract(index: &serde_json::Value) -> String {
    let Some(map) = index.as_object() else { return String::new() };
    let mut words: Vec<(u64, &str)> = Vec::new();
    for (word, positions) in map {
        for position in positions.as_array().into_iter().flatten() {
            if let Some(at) = position.as_u64() {
                words.push((at, word.as_str()));
            }
        }
    }
    words.sort_unstable_by_key(|(at, _)| *at);
    words.into_iter().map(|(_, w)| w).collect::<Vec<_>>().join(" ")
}

/// Crossref and PubMed both put markup in their abstracts and titles.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of the first `<name>…</name>`, unescaped.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let start = xml.find(&open)?;
    let after = xml[start..].find('>')? + start + 1;
    let end = xml[after..].find(&format!("</{name}>"))? + after;
    let text = unescape(&xml[after..end]);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// arXiv puts the DOI in `<arxiv:doi>`, which the tag reader would find under
/// its own name only if asked for the prefixed one.
fn attribute_of(xml: &str, name: &str) -> Option<String> {
    tag(xml, &format!("arxiv:{name}"))
}

fn unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Every `start … end` slice, exclusive of the terminator.
fn between_all<'a>(text: &'a str, start: &'a str, end: &'a str) -> impl Iterator<Item = &'a str> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let from = text[at..].find(start)? + at;
        let to = text[from..].find(end).map(|e| from + e)?;
        at = to + end.len();
        Some(&text[from..to])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ARXIV: &str = r#"<feed>
      <entry>
        <id>http://arxiv.org/abs/1706.03762v7</id>
        <published>2017-06-12T17:57:34Z</published>
        <title>Attention Is All You Need</title>
        <summary>The dominant sequence transduction models are based on
        complex recurrent networks.</summary>
        <author><name>Ashish Vaswani</name></author>
        <author><name>Noam Shazeer</name></author>
      </entry>
      <entry>
        <id>http://arxiv.org/abs/2005.14165v4</id>
        <published>2020-05-28T00:00:00Z</published>
        <title>Language Models are Few-Shot Learners</title>
        <author><name>Tom B. Brown</name></author>
      </entry>
    </feed>"#;

    #[test]
    fn an_arxiv_feed_becomes_results() {
        let found = arxiv_feed(ARXIV, 10);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Attention Is All You Need");
        assert_eq!(found[0].authors, vec!["Ashish Vaswani", "Noam Shazeer"]);
        assert_eq!(found[0].year, Some(2017));
        // The number, not the version: 1706.03762 and v7 are one paper.
        assert_eq!(found[0].identifier.as_deref(), Some("1706.03762v7"));
        assert!(found[0].summary.as_ref().unwrap().contains("sequence transduction"));
    }

    /// A title broken across lines in the XML is one title, not two.
    #[test]
    fn wrapped_text_is_joined_rather_than_kept_as_layout() {
        let found = arxiv_feed(ARXIV, 10);
        assert!(!found[0].summary.as_ref().unwrap().contains('\n'));
        assert!(found[0].summary.as_ref().unwrap().contains("complex recurrent"));
    }

    #[test]
    fn a_limit_is_a_limit() {
        assert_eq!(arxiv_feed(ARXIV, 1).len(), 1);
    }

    #[test]
    fn an_empty_feed_is_no_results_rather_than_a_panic() {
        assert!(arxiv_feed("<feed></feed>", 10).is_empty());
        assert!(arxiv_feed("", 10).is_empty());
    }

    #[test]
    fn a_crossref_work_becomes_a_result() {
        let body = json!({"message": {"items": [{
            "title": ["Attention Is All You Need"],
            "DOI": "10.5555/3295222.3295349",
            "URL": "https://doi.org/10.5555/3295222.3295349",
            "container-title": ["NeurIPS"],
            "issued": {"date-parts": [[2017, 6]]},
            "author": [{"given": "Ashish", "family": "Vaswani"}],
            "abstract": "<jats:p>The dominant models.</jats:p>"
        }]}});
        let found = crossref_works(&body, 10);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].identifier.as_deref(), Some("10.5555/3295222.3295349"));
        assert_eq!(found[0].venue.as_deref(), Some("NeurIPS"));
        assert_eq!(found[0].year, Some(2017));
        assert_eq!(found[0].authors, vec!["Ashish Vaswani"]);
        // Publishers put markup in abstracts; a reader should not see it.
        assert_eq!(found[0].summary.as_deref(), Some("The dominant models."));
    }

    /// A work with no title is not a result. Crossref indexes components --
    /// figures, chapters -- that have none.
    #[test]
    fn an_untitled_work_is_left_out() {
        let body = json!({"message": {"items": [{"DOI": "10.1/x"}, {"title": [""], "DOI": "10.1/y"}]}});
        assert!(crossref_works(&body, 10).is_empty());
    }

    #[test]
    fn an_openalex_work_becomes_a_result() {
        let body = json!({"results": [{
            "display_name": "Deep learning",
            "doi": "https://doi.org/10.1038/nature14539",
            "publication_year": 2015,
            "primary_location": {
                "source": {"display_name": "Nature"},
                "landing_page_url": "https://www.nature.com/articles/nature14539"
            },
            "authorships": [{"author": {"display_name": "Yann LeCun"}}],
            "abstract_inverted_index": {"Deep": [0], "learning": [1], "works": [2]}
        }]});
        let found = openalex_works(&body, 10);
        assert_eq!(found.len(), 1);
        // The bare DOI, not the URL form: that is what resolves.
        assert_eq!(found[0].identifier.as_deref(), Some("10.1038/nature14539"));
        assert_eq!(found[0].venue.as_deref(), Some("Nature"));
        // OpenAlex inverts abstracts to sidestep copyright; putting the words
        // back in order is the only way to read one.
        assert_eq!(found[0].summary.as_deref(), Some("Deep learning works"));
    }

    #[test]
    fn a_pubmed_summary_becomes_a_result() {
        let body = json!({"result": {
            "uids": ["31978945"],
            "31978945": {
                "title": "A Novel Coronavirus from Patients with Pneumonia in China, 2019",
                "pubdate": "2020 Feb 20",
                "fulljournalname": "The New England Journal of Medicine",
                "authors": [{"name": "Zhu N"}, {"name": "Zhang D"}],
                "articleids": [
                    {"idtype": "pubmed", "value": "31978945"},
                    {"idtype": "doi", "value": "10.1056/NEJMoa2001017"}
                ]
            }
        }});
        let found = pubmed_summaries(&body, 10);
        assert_eq!(found.len(), 1);
        // The DOI, not the PubMed page: it resolves to the version of record.
        assert_eq!(found[0].identifier.as_deref(), Some("10.1056/NEJMoa2001017"));
        assert_eq!(found[0].url.as_deref(), Some("https://pubmed.ncbi.nlm.nih.gov/31978945/"));
        assert_eq!(found[0].year, Some(2020));
        assert_eq!(found[0].authors, vec!["Zhu N", "Zhang D"]);
    }

    /// Without a DOI the PubMed page is still an address that resolves, so the
    /// result stays addable rather than being dropped.
    #[test]
    fn a_pubmed_record_with_no_doi_still_has_something_to_add_it_by() {
        let body = json!({"result": {
            "uids": ["1"],
            "1": {"title": "An old paper", "pubdate": "1974"}
        }});
        let found = pubmed_summaries(&body, 10);
        assert_eq!(found[0].identifier.as_deref(), Some("https://pubmed.ncbi.nlm.nih.gov/1/"));
    }

    #[test]
    fn a_malformed_payload_is_no_results_rather_than_a_panic() {
        for empty in [json!({}), json!({"message": {}}), json!({"result": {"uids": []}}), json!(null)] {
            assert!(crossref_works(&empty, 10).is_empty());
            assert!(openalex_works(&empty, 10).is_empty());
            assert!(pubmed_summaries(&empty, 10).is_empty());
        }
    }
}
