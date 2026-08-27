//! Handing the library to another program.
//!
//! Rendering a citation and exporting a record look similar and are not. A
//! citation is for a reader: it obeys a style, drops what the style does not
//! want, and its output is prose. An export is for a machine: it keeps
//! everything it can, obeys a grammar rather than a taste, and something else
//! will parse it back. They live side by side here because both turn an [`Item`]
//! into text, but nothing in one should ever be reused to "simplify" the other.
//!
//! Three formats, chosen by what people actually paste where: BibTeX for LaTeX,
//! RIS for EndNote and most publisher sites, CSL-JSON for anything modern —
//! Pandoc especially. Zotero RDF is deliberately absent: it is an XML dialect
//! nothing else reads, and claiming support for it without a parser to check
//! against would be a promise this cannot keep.

use yk_core::model::{Creator, Item};

/// What to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    BibTeX,
    Ris,
    CslJson,
}

impl Export {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "bibtex" | "bib" => Some(Self::BibTeX),
            "ris" => Some(Self::Ris),
            "csljson" | "csl-json" | "json" => Some(Self::CslJson),
            _ => None,
        }
    }

    /// What a browser should call the file it just downloaded.
    pub fn extension(self) -> &'static str {
        match self {
            Self::BibTeX => "bib",
            Self::Ris => "ris",
            Self::CslJson => "json",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            // `application/x-bibtex` is what publishers serve and what
            // reference managers sniff for.
            Self::BibTeX => "application/x-bibtex",
            Self::Ris => "application/x-research-info-systems",
            Self::CslJson => "application/json",
        }
    }
}

/// Write a whole set of items.
pub fn export(items: &[Item], format: Export) -> String {
    match format {
        Export::BibTeX => items.iter().map(bibtex).collect::<Vec<_>>().join("\n"),
        Export::Ris => items.iter().map(ris).collect::<Vec<_>>().join("\n"),
        Export::CslJson => {
            let entries: Vec<serde_json::Value> = items.iter().map(csl).collect();
            serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".into())
        }
    }
}

// ---------------------------------------------------------------------------
// BibTeX
// ---------------------------------------------------------------------------

/// The BibTeX entry type for one of ours.
///
/// `misc` is the honest fallback: an entry type BibTeX does not know makes the
/// whole file fail to compile, and a paper that will not build is worse than a
/// web page filed as `misc`.
fn bibtex_type(item_type: &str) -> &'static str {
    match item_type {
        "journalArticle" | "magazineArticle" | "newspaperArticle" => "article",
        "book" => "book",
        "bookSection" => "incollection",
        "conferencePaper" => "inproceedings",
        "thesis" => "phdthesis",
        "report" => "techreport",
        "manuscript" | "preprint" => "unpublished",
        _ => "misc",
    }
}

fn bibtex(item: &Item) -> String {
    let mut out = format!("@{}{{{},\n", bibtex_type(&item.item_type), cite_key(item));

    let mut field = |name: &str, value: &str| {
        if !value.trim().is_empty() {
            out.push_str(&format!("  {name} = {{{}}},\n", brace_escape(value)));
        }
    };

    let authors = role(item, "author");
    if !authors.is_empty() {
        // BibTeX's own separator. A comma would make "Zhang, Wei" two people.
        field("author", &authors.join(" and "));
    }
    let editors = role(item, "editor");
    if !editors.is_empty() {
        field("editor", &editors.join(" and "));
    }

    field("title", item.title());
    match item.item_type.as_str() {
        "bookSection" | "conferencePaper" => field("booktitle", container(item)),
        _ => field("journal", container(item)),
    }
    field("year", &crate::year(item));
    field("volume", item.field("volume").unwrap_or_default());
    field("number", item.field("issue").unwrap_or_default());
    field("pages", &item.field("pages").unwrap_or_default().replace('-', "--"));
    field("publisher", item.field("publisher").unwrap_or_default());
    field("institution", item.field("institution").unwrap_or_default());
    field("school", item.field("university").unwrap_or_default());
    field("doi", item.field("DOI").unwrap_or_default());
    field("isbn", item.field("ISBN").unwrap_or_default());
    field("url", item.field("url").unwrap_or_default());
    field("abstract", item.field("abstractNote").unwrap_or_default());
    let tags: Vec<&str> = item.tags.iter().map(|t| t.tag.as_str()).collect();
    if !tags.is_empty() {
        field("keywords", &tags.join(", "));
    }

    out.push_str("}\n");
    out
}

/// `zhang2020attention`: author, year, and the first real word of the title.
///
/// Stable and readable, which is what a key is for — somebody types it into a
/// `\cite{}` by hand. Uniqueness within one export is not attempted: BibTeX
/// tolerates duplicates by taking the first, and inventing `zhang2020a` for
/// records that may be genuinely different papers would be a guess.
fn cite_key(item: &Item) -> String {
    let who = item
        .creators
        .iter()
        .find(|c| c.creator_type == "author")
        .map(Creator::sort_name)
        .unwrap_or_default();
    let word = item
        .title()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .find(|w| w.len() > 3)
        .unwrap_or("");

    let clean = |s: &str| -> String {
        s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
    };
    let key = format!("{}{}{}", clean(&who), crate::year(item), clean(word));
    if key.is_empty() {
        item.key.to_string().to_lowercase()
    } else {
        key
    }
}

/// Make a value safe inside `{…}`.
///
/// The characters that matter are the ones that end the field early or start
/// something: an unbalanced brace truncates the entry, and a bare `%` comments
/// out the rest of the line — both of which break the *file*, not just the
/// entry, which is why an export that does this wrong is worse than no export.
fn brace_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        match c {
            '{' | '}' | '%' | '$' | '&' | '#' | '_' => {
                out.push('\\');
                out.push(c);
            }
            '\\' => out.push_str("\\textbackslash{}"),
            // A newline inside a field is legal and unreadable; BibTeX treats
            // runs of whitespace as one space anyway.
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// RIS
// ---------------------------------------------------------------------------

fn ris_type(item_type: &str) -> &'static str {
    match item_type {
        "journalArticle" => "JOUR",
        "magazineArticle" => "MGZN",
        "newspaperArticle" => "NEWS",
        "book" => "BOOK",
        "bookSection" => "CHAP",
        "conferencePaper" => "CONF",
        "thesis" => "THES",
        "report" => "RPRT",
        "webpage" => "ELEC",
        _ => "GEN",
    }
}

fn ris(item: &Item) -> String {
    let mut out = format!("TY  - {}\n", ris_type(&item.item_type));
    let mut tag = |name: &str, value: &str| {
        if !value.trim().is_empty() {
            // RIS is line-oriented: a value with a newline in it becomes a tag
            // the parser cannot read.
            out.push_str(&format!("{name}  - {}\n", value.replace(['\n', '\r'], " ")));
        }
    };

    for author in role(item, "author") {
        tag("AU", &author);
    }
    for editor in role(item, "editor") {
        tag("ED", &editor);
    }
    tag("TI", item.title());
    tag("T2", container(item));
    tag("PY", &crate::year(item));
    // The full date as well, when there is more of it than a year.
    let date = item.field("date").unwrap_or_default();
    if date.len() > 4 {
        tag("DA", date);
    }
    tag("VL", item.field("volume").unwrap_or_default());
    tag("IS", item.field("issue").unwrap_or_default());

    let pages = item.field("pages").unwrap_or_default();
    if let Some((from, to)) = pages.split_once(['-', '–']) {
        tag("SP", from.trim());
        tag("EP", to.trim_start_matches(['-', '–']).trim());
    } else {
        tag("SP", pages);
    }

    tag("PB", item.field("publisher").unwrap_or_default());
    tag("DO", item.field("DOI").unwrap_or_default());
    tag("SN", item.field("ISSN").or(item.field("ISBN")).unwrap_or_default());
    tag("UR", item.field("url").unwrap_or_default());
    tag("AB", item.field("abstractNote").unwrap_or_default());
    for t in &item.tags {
        tag("KW", &t.tag);
    }

    // Every record ends with `ER`, and a file whose last record does not is
    // silently dropped by most importers.
    out.push_str("ER  - \n");
    out
}

// ---------------------------------------------------------------------------
// CSL-JSON
// ---------------------------------------------------------------------------

fn csl_type(item_type: &str) -> &'static str {
    match item_type {
        "journalArticle" => "article-journal",
        "magazineArticle" => "article-magazine",
        "newspaperArticle" => "article-newspaper",
        "book" => "book",
        "bookSection" => "chapter",
        "conferencePaper" => "paper-conference",
        "thesis" => "thesis",
        "report" => "report",
        "webpage" => "webpage",
        _ => "document",
    }
}

fn csl(item: &Item) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    out.insert("id".into(), cite_key(item).into());
    out.insert("type".into(), csl_type(&item.item_type).into());

    let mut put = |name: &str, value: &str| {
        if !value.trim().is_empty() {
            out.insert(name.into(), value.into());
        }
    };
    put("title", item.title());
    put("container-title", container(item));
    put("volume", item.field("volume").unwrap_or_default());
    put("issue", item.field("issue").unwrap_or_default());
    put("page", item.field("pages").unwrap_or_default());
    put("publisher", item.field("publisher").unwrap_or_default());
    put("DOI", item.field("DOI").unwrap_or_default());
    put("ISBN", item.field("ISBN").unwrap_or_default());
    put("URL", item.field("url").unwrap_or_default());
    put("abstract", item.field("abstractNote").unwrap_or_default());

    let names = |kind: &str| -> Vec<serde_json::Value> {
        item.creators
            .iter()
            .filter(|c| c.creator_type == kind)
            .map(|c| {
                let mut who = serde_json::Map::new();
                // A one-field name is `literal`, not a family name: CSL styles
                // that abbreviate given names would otherwise turn an
                // institution into an initial.
                if let Some(name) = &c.name {
                    who.insert("literal".into(), name.clone().into());
                } else {
                    if let Some(last) = &c.last_name {
                        who.insert("family".into(), last.clone().into());
                    }
                    if let Some(first) = &c.first_name {
                        who.insert("given".into(), first.clone().into());
                    }
                }
                serde_json::Value::Object(who)
            })
            .collect()
    };
    for kind in ["author", "editor"] {
        let people = names(kind);
        if !people.is_empty() {
            out.insert(kind.into(), people.into());
        }
    }

    let year = crate::year(item);
    if !year.is_empty() {
        if let Ok(y) = year.parse::<i64>() {
            out.insert(
                "issued".into(),
                serde_json::json!({ "date-parts": [[y]] }),
            );
        }
    }

    serde_json::Value::Object(out)
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// The people of one kind, written the way a bibliography writes them.
fn role(item: &Item, kind: &str) -> Vec<String> {
    item.creators
        .iter()
        .filter(|c| c.creator_type == kind)
        .map(|c| match (&c.name, &c.last_name, &c.first_name) {
            // Indivisible names travel whole. Splitting 张伟 on its space —
            // there is none — or guessing a given name would invent a person.
            (Some(name), _, _) => name.clone(),
            (None, Some(last), Some(first)) => format!("{last}, {first}"),
            (None, Some(last), None) => last.clone(),
            (None, None, Some(first)) => first.clone(),
            _ => String::new(),
        })
        .filter(|s| !s.trim().is_empty())
        .collect()
}

/// What the work appeared in, whatever the item type calls it.
fn container(item: &Item) -> &str {
    item.field("publicationTitle")
        .or_else(|| item.field("bookTitle"))
        .or_else(|| item.field("proceedingsTitle"))
        .or_else(|| item.field("websiteTitle"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::model::{ItemDraft, ItemTag};
    use yk_core::Key;

    fn author(last: &str, first: &str) -> Creator {
        Creator {
            creator_type: "author".into(),
            last_name: Some(last.into()),
            first_name: Some(first.into()),
            name: None,
        }
    }

    fn paper() -> Item {
        ItemDraft::new("journalArticle")
            .with_field("title", "Attention Is All You Need")
            .with_field("publicationTitle", "Advances in Neural Information Processing")
            .with_field("date", "2017-06-12")
            .with_field("volume", "30")
            .with_field("issue", "2")
            .with_field("pages", "5998-6008")
            .with_field("DOI", "10.5555/3295222")
            .with_creator(author("Vaswani", "Ashish"))
            .with_creator(author("Shazeer", "Noam"))
            .into_item(Key::generate(), 1, 1)
    }

    #[test]
    fn bibtex_writes_an_entry_latex_can_read() {
        let out = export(&[paper()], Export::BibTeX);
        assert!(out.starts_with("@article{vaswani2017attention,"));
        assert!(out.contains("author = {Vaswani, Ashish and Shazeer, Noam}"), "{out}");
        assert!(out.contains("journal = {Advances in Neural Information Processing}"));
        assert!(out.contains("year = {2017}"));
        // An en dash between page numbers is what LaTeX expects.
        assert!(out.contains("pages = {5998--6008}"));
        assert!(out.trim_end().ends_with('}'));
    }

    #[test]
    fn bibtex_escapes_what_would_break_the_file() {
        // Not the entry — the *file*. A bare `%` comments out the rest of the
        // line and an unbalanced brace swallows everything after it, so one bad
        // title takes the whole bibliography with it.
        let item = ItemDraft::new("journalArticle")
            .with_field("title", "100% coverage of {some} things & more_stuff")
            .with_creator(author("Zhang", "Wei"))
            .into_item(Key::generate(), 1, 1);

        let out = export(&[item], Export::BibTeX);
        assert!(out.contains("100\\% coverage of \\{some\\} things \\& more\\_stuff"), "{out}");
    }

    #[test]
    fn a_chapter_goes_in_a_booktitle_not_a_journal() {
        let item = ItemDraft::new("bookSection")
            .with_field("title", "A Chapter")
            .with_field("bookTitle", "A Book Of Chapters")
            .into_item(Key::generate(), 1, 1);

        let out = export(&[item], Export::BibTeX);
        assert!(out.starts_with("@incollection{"));
        assert!(out.contains("booktitle = {A Book Of Chapters}"));
        assert!(!out.contains("journal ="));
    }

    #[test]
    fn an_unknown_type_still_compiles() {
        // `@interview` is not a BibTeX entry type, and an unknown one fails the
        // whole document rather than the entry.
        let item = ItemDraft::new("interview")
            .with_field("title", "A Conversation")
            .into_item(Key::generate(), 1, 1);
        assert!(export(&[item], Export::BibTeX).starts_with("@misc{"));
    }

    #[test]
    fn ris_splits_the_page_range_and_ends_the_record() {
        let out = export(&[paper()], Export::Ris);
        assert!(out.starts_with("TY  - JOUR\n"));
        assert!(out.contains("AU  - Vaswani, Ashish\n"));
        assert!(out.contains("AU  - Shazeer, Noam\n"), "one line each, not one line");
        assert!(out.contains("SP  - 5998\n"));
        assert!(out.contains("EP  - 6008\n"));
        assert!(out.contains("PY  - 2017\n"));
        assert!(out.contains("DA  - 2017-06-12\n"), "the full date as well as the year");
        assert!(out.trim_end().ends_with("ER  -"), "a record with no ER is dropped on import");
    }

    #[test]
    fn ris_keeps_every_record_separately_terminated() {
        let out = export(&[paper(), paper()], Export::Ris);
        assert_eq!(out.matches("TY  - ").count(), 2);
        assert_eq!(out.matches("ER  - ").count(), 2);
    }

    #[test]
    fn csl_json_is_what_pandoc_expects() {
        let out = export(&[paper()], Export::CslJson);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let first = &parsed[0];

        assert_eq!(first["type"], "article-journal");
        assert_eq!(first["container-title"], "Advances in Neural Information Processing");
        assert_eq!(first["author"][0]["family"], "Vaswani");
        assert_eq!(first["author"][0]["given"], "Ashish");
        assert_eq!(first["issued"]["date-parts"][0][0], 2017);
        assert_eq!(first["DOI"], "10.5555/3295222");
    }

    #[test]
    fn an_indivisible_name_is_never_split_into_parts() {
        // An institution, or most CJK names. Written as a family name, a style
        // that abbreviates given names turns it into an initial.
        let item = ItemDraft::new("report")
            .with_field("title", "Annual Report")
            .with_creator(Creator {
                creator_type: "author".into(),
                name: Some("World Health Organization".into()),
                last_name: None,
                first_name: None,
            })
            .into_item(Key::generate(), 1, 1);

        assert!(export(std::slice::from_ref(&item), Export::BibTeX)
            .contains("author = {World Health Organization}"));
        assert!(export(std::slice::from_ref(&item), Export::Ris).contains("AU  - World Health Organization"));

        let csl: serde_json::Value =
            serde_json::from_str(&export(&[item], Export::CslJson)).unwrap();
        assert_eq!(csl[0]["author"][0]["literal"], "World Health Organization");
        assert!(csl[0]["author"][0]["family"].is_null());
    }

    #[test]
    fn empty_fields_are_left_out_rather_than_written_blank() {
        // An importer reads `volume = {}` as a volume.
        let bare = ItemDraft::new("journalArticle")
            .with_field("title", "Nothing Else Known")
            .into_item(Key::generate(), 1, 1);

        let out = export(std::slice::from_ref(&bare), Export::BibTeX);
        assert!(!out.contains("{}"), "{out}");
        assert!(!export(&[bare], Export::Ris).contains("VL  -"));
    }

    #[test]
    fn tags_travel_as_keywords() {
        let mut item = paper();
        item.tags = vec![ItemTag::manual("transformers"), ItemTag::manual("nlp")];
        assert!(export(std::slice::from_ref(&item), Export::BibTeX).contains("keywords = {transformers, nlp}"));
        assert!(export(&[item], Export::Ris).contains("KW  - transformers\n"));
    }

    #[test]
    fn a_format_is_recognised_by_the_names_people_use_for_it() {
        assert_eq!(Export::parse("BibTeX"), Some(Export::BibTeX));
        assert_eq!(Export::parse("bib"), Some(Export::BibTeX));
        assert_eq!(Export::parse(" ris "), Some(Export::Ris));
        assert_eq!(Export::parse("csl-json"), Some(Export::CslJson));
        assert_eq!(Export::parse("zotero-rdf"), None, "not supported, and it must say so");
    }
}
