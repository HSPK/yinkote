//! Reading BibTeX and RIS back in.
//!
//! The other half of [`crate::export`], and the way most references arrive:
//! every publisher's "download citation" button hands over one of these two,
//! and a manager that cannot read them makes people retype.
//!
//! Parsing is forgiving on purpose. These files are written by hundreds of
//! programs and a good many of them are wrong — a missing final brace, a `year`
//! field holding `2019a`, an RIS record that never says `ER`. Refusing a file
//! of forty references because the eleventh is malformed helps nobody, so a
//! record that cannot be read is reported and the rest are kept.

use yk_core::model::{Creator, ItemDraft};

/// A record that could not be read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// 1-based, counting records rather than lines: "the third entry" is what
    /// somebody can find in their file.
    pub index: usize,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub items: Vec<ItemDraft>,
    pub rejected: Vec<Rejected>,
}

/// Work out which format a file is and read it.
///
/// Sniffed rather than asked for: the extension is often wrong, and a user who
/// has just downloaded `citation.txt` should not have to know.
pub fn parse(text: &str) -> Parsed {
    if looks_like_ris(text) {
        ris(text)
    } else {
        bibtex(text)
    }
}

/// RIS records begin with a type tag and end with `ER`.
fn looks_like_ris(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("TY  - ") || l.starts_with("TY - ")
    })
}

// ---------------------------------------------------------------------------
// BibTeX
// ---------------------------------------------------------------------------

fn item_type_for_bibtex(entry: &str) -> &'static str {
    match entry.to_ascii_lowercase().as_str() {
        "article" => "journalArticle",
        "book" | "booklet" => "book",
        "incollection" | "inbook" => "bookSection",
        "inproceedings" | "conference" | "proceedings" => "conferencePaper",
        "phdthesis" | "mastersthesis" => "thesis",
        "techreport" => "report",
        "unpublished" => "manuscript",
        _ => "document",
    }
}

pub fn bibtex(text: &str) -> Parsed {
    let mut out = Parsed::default();
    for (index, raw) in split_bibtex(text).into_iter().enumerate() {
        match bibtex_entry(&raw) {
            Ok(draft) => out.items.push(draft),
            Err(reason) => out.rejected.push(Rejected { index: index + 1, reason }),
        }
    }
    out
}

/// Cut a file into entries at each top-level `@`.
///
/// Brace-counting rather than splitting on `@`: an `@` inside a title or an
/// email address is common, and splitting there truncates the entry before it.
fn split_bibtex(text: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let start = i;
        let mut depth = 0usize;
        let mut opened = false;
        let mut in_quotes = false;
        while i < chars.len() {
            match chars[i] {
                // An escaped brace is a character, not a nesting level.
                '\\' => i += 1,
                '"' if depth <= 1 => in_quotes = !in_quotes,
                '{' if !in_quotes => {
                    depth += 1;
                    opened = true;
                }
                '}' if !in_quotes => {
                    depth = depth.saturating_sub(1);
                    if opened && depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if opened {
            entries.push(chars[start..i.min(chars.len())].iter().collect());
        }
    }
    entries
}

fn bibtex_entry(raw: &str) -> Result<ItemDraft, String> {
    let body = raw.trim_start_matches('@');
    let open = body.find('{').ok_or_else(|| "no opening brace".to_string())?;
    let entry_type = body[..open].trim();
    if entry_type.is_empty() {
        return Err("no entry type".into());
    }
    // `@comment` and `@string` are not references.
    if matches!(entry_type.to_ascii_lowercase().as_str(), "comment" | "string" | "preamble") {
        return Err(format!("@{entry_type} is not a reference"));
    }

    let inner = body[open + 1..].trim_end().trim_end_matches('}');
    let mut draft = ItemDraft::new(item_type_for_bibtex(entry_type));
    let mut seen_any = false;
    // Held rather than written as they arrive: BibTeX keeps the year and the
    // month in separate fields, and the library stores one date. Exporting and
    // re-importing used to return `2021` for a paper published in March,
    // because `month` was read and dropped.
    let mut month: Option<u32> = None;

    for (name, value) in bibtex_fields(inner) {
        let value = unbrace(&value);
        if value.is_empty() {
            continue;
        }
        seen_any = true;
        match name.as_str() {
            "author" => {
                for person in value.split(" and ") {
                    if let Some(c) = creator(person.trim(), "author") {
                        draft = draft.with_creator(c);
                    }
                }
            }
            "editor" => {
                for person in value.split(" and ") {
                    if let Some(c) = creator(person.trim(), "editor") {
                        draft = draft.with_creator(c);
                    }
                }
            }
            "title" => draft = draft.with_field("title", value),
            "journal" | "journaltitle" => draft = draft.with_field("publicationTitle", value),
            "booktitle" => draft = draft.with_field("bookTitle", value),
            "year" => draft = draft.with_field("date", value),
            "month" => month = bibtex_month(&value),
            "volume" => draft = draft.with_field("volume", value),
            "number" => draft = draft.with_field("issue", value),
            // `10--20` is BibTeX's en dash; the library stores a plain range.
            "pages" => draft = draft.with_field("pages", value.replace("--", "-")),
            "publisher" => draft = draft.with_field("publisher", value),
            "institution" | "organization" => draft = draft.with_field("institution", value),
            "school" => draft = draft.with_field("university", value),
            "doi" => draft = draft.with_field("DOI", value),
            "isbn" => draft = draft.with_field("ISBN", value),
            "issn" => draft = draft.with_field("ISSN", value),
            "url" => draft = draft.with_field("url", value),
            "abstract" => draft = draft.with_field("abstractNote", value),
            "keywords" => {
                for tag in value.split(&[',', ';'][..]) {
                    let tag = tag.trim();
                    if !tag.is_empty() {
                        draft.tags.push(yk_core::model::ItemTag::manual(tag));
                    }
                }
            }
            _ => {}
        }
    }

    if !seen_any {
        return Err("no fields".into());
    }
    // Only when the year is a bare year: anything richer already carries more
    // than the month would add, and overwriting it would lose the day.
    let bare_year = draft
        .fields
        .get("date")
        .and_then(|d| d.as_str())
        .filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string);
    if let (Some(m), Some(year)) = (month, bare_year) {
        draft = draft.with_field("date", format!("{year}-{m:02}").as_str());
    }
    // A reference with neither a title nor an author is not one; importing it
    // makes a blank row somebody has to find and delete.
    if draft.fields.get("title").is_none() && draft.creators.is_empty() {
        return Err("neither a title nor an author".into());
    }
    Ok(draft)
}

/// BibTeX months: the three-letter macros, the full names, and plain numbers.
///
/// Anything else is left alone rather than guessed at — a month nobody can
/// read is a smaller loss than one read wrongly.
fn bibtex_month(value: &str) -> Option<u32> {
    let text = value.trim().to_ascii_lowercase();
    if let Ok(n) = text.parse::<u32>() {
        return (1..=12).contains(&n).then_some(n);
    }
    const NAMES: [&str; 12] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september",
        "october", "november", "december",
    ];
    NAMES
        .iter()
        .position(|full| full.starts_with(&text) && text.len() >= 3)
        .map(|i| i as u32 + 1)
}

/// `name = value` pairs, splitting on the commas that are between fields.
fn bibtex_fields(inner: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let mut depth = 0usize;
    let mut in_quotes = false;
    let mut field = String::new();

    let flush = |field: &mut String, out: &mut Vec<(String, String)>| {
        if let Some((name, value)) = field.split_once('=') {
            let name = name.trim().to_ascii_lowercase();
            if !name.is_empty() {
                out.push((name, value.trim().to_string()));
            }
        }
        field.clear();
    };

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                field.push(c);
                field.push(chars[i + 1]);
                i += 2;
                continue;
            }
            '{' => {
                depth += 1;
                field.push(c);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                field.push(c);
            }
            '"' if depth == 0 => {
                in_quotes = !in_quotes;
                field.push(c);
            }
            ',' if depth == 0 && !in_quotes => flush(&mut field, &mut out),
            _ => field.push(c),
        }
        i += 1;
    }
    flush(&mut field, &mut out);

    // The first "field" is the cite key, which has no `=` and is dropped by
    // `split_once` above.
    out
}

/// Strip the wrapping braces or quotes and undo the escaping.
fn unbrace(value: &str) -> String {
    let mut v = value.trim();
    while (v.starts_with('{') && v.ends_with('}')) || (v.starts_with('"') && v.ends_with('"')) {
        v = &v[1..v.len() - 1];
        v = v.trim();
    }
    let mut out = String::with_capacity(v.len());
    let chars: Vec<char> = v.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                '%' | '$' | '&' | '#' | '_' | '{' | '}' => {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Braces that survive are BibTeX's "keep this capitalised" markers and
        // mean nothing once the text is stored.
        if chars[i] != '{' && chars[i] != '}' {
            out.push(chars[i]);
        }
        i += 1;
    }
    out.replace("\\textbackslash", "\\").split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// RIS
// ---------------------------------------------------------------------------

fn item_type_for_ris(ty: &str) -> &'static str {
    match ty.trim().to_ascii_uppercase().as_str() {
        "JOUR" => "journalArticle",
        "MGZN" => "magazineArticle",
        "NEWS" => "newspaperArticle",
        "BOOK" => "book",
        "CHAP" => "bookSection",
        "CONF" | "CPAPER" => "conferencePaper",
        "THES" => "thesis",
        "RPRT" => "report",
        "ELEC" | "ICOMM" => "webpage",
        _ => "document",
    }
}

pub fn ris(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut current: Vec<(String, String)> = Vec::new();
    let mut index = 0usize;
    let mut last_tag: Option<usize> = None;

    for line in text.lines() {
        let line = line.trim_end();
        match ris_tag(line) {
            Some((tag, value)) => {
                if tag == "ER" {
                    index += 1;
                    finish_ris(&mut current, index, &mut out);
                    last_tag = None;
                    continue;
                }
                if tag == "TY" && !current.is_empty() {
                    // A file whose records never say `ER`. Common enough to be
                    // worth handling: the next `TY` ends the previous record.
                    index += 1;
                    finish_ris(&mut current, index, &mut out);
                }
                current.push((tag, value));
                last_tag = Some(current.len() - 1);
            }
            // A continuation line: RIS wraps long abstracts without repeating
            // the tag, and dropping them loses most of the abstract.
            None if !line.trim().is_empty() => {
                if let Some(i) = last_tag {
                    current[i].1.push(' ');
                    current[i].1.push_str(line.trim());
                }
            }
            None => {}
        }
    }
    if !current.is_empty() {
        index += 1;
        finish_ris(&mut current, index, &mut out);
    }
    out
}

fn ris_tag(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    if bytes.len() < 4 || !bytes[..2].iter().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    // `TY  - JOUR` is the standard, but plenty of writers emit `TY - JOUR`.
    let rest = line.get(2..)?;
    let rest = rest.strip_prefix("  - ").or_else(|| rest.strip_prefix(" - ")).or_else(|| {
        // `ER  -` with nothing after it, and no trailing space.
        rest.trim_end().strip_suffix('-').filter(|r| r.trim().is_empty()).map(|_| "")
    })?;
    Some((line[..2].to_ascii_uppercase(), rest.trim().to_string()))
}

fn finish_ris(fields: &mut Vec<(String, String)>, index: usize, out: &mut Parsed) {
    let taken = std::mem::take(fields);
    let ty = taken
        .iter()
        .find(|(t, _)| t == "TY")
        .map(|(_, v)| v.as_str())
        .unwrap_or("GEN");
    let mut draft = ItemDraft::new(item_type_for_ris(ty));
    let mut start = String::new();
    let mut end = String::new();

    for (tag, value) in &taken {
        if value.is_empty() {
            continue;
        }
        match tag.as_str() {
            "AU" | "A1" => {
                if let Some(c) = creator(value, "author") {
                    draft = draft.with_creator(c);
                }
            }
            "ED" | "A2" => {
                if let Some(c) = creator(value, "editor") {
                    draft = draft.with_creator(c);
                }
            }
            "TI" | "T1" => draft = draft.with_field("title", value.clone()),
            "T2" | "JO" | "JF" => draft = draft.with_field("publicationTitle", value.clone()),
            // `DA` carries the full date and `PY` only the year, so a later
            // `DA` should win — the loop order does that, since `DA` is
            // conventionally written after `PY`.
            "PY" | "Y1" | "DA" => draft = draft.with_field("date", value.clone()),
            "VL" => draft = draft.with_field("volume", value.clone()),
            "IS" => draft = draft.with_field("issue", value.clone()),
            "SP" => start = value.clone(),
            "EP" => end = value.clone(),
            "PB" => draft = draft.with_field("publisher", value.clone()),
            "DO" => draft = draft.with_field("DOI", value.clone()),
            "SN" => draft = draft.with_field("ISSN", value.clone()),
            "UR" | "L1" => draft = draft.with_field("url", value.clone()),
            "AB" | "N2" => draft = draft.with_field("abstractNote", value.clone()),
            "KW" => draft.tags.push(yk_core::model::ItemTag::manual(value.trim())),
            _ => {}
        }
    }

    if !start.is_empty() {
        let pages = if end.is_empty() { start } else { format!("{start}-{end}") };
        draft = draft.with_field("pages", pages);
    }

    if draft.fields.get("title").is_none() && draft.creators.is_empty() {
        out.rejected.push(Rejected { index, reason: "neither a title nor an author".into() });
        return;
    }
    out.items.push(draft);
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// Read one person, in either of the two shapes these formats use.
///
/// `Zhang, Wei` splits; `World Health Organization` does not. The distinction
/// is the comma, and guessing without one turns an institution into a surname
/// with a given name invented from the rest of its name.
fn creator(raw: &str, kind: &str) -> Option<Creator> {
    let raw = raw.trim().trim_end_matches(',').trim();
    if raw.is_empty() {
        return None;
    }
    Some(match raw.split_once(',') {
        Some((last, first)) => Creator {
            creator_type: kind.into(),
            last_name: Some(last.trim().to_string()),
            first_name: Some(first.trim().to_string()).filter(|f| !f.is_empty()),
            name: None,
        },
        None => Creator {
            creator_type: kind.into(),
            last_name: None,
            first_name: None,
            name: Some(raw.to_string()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{export, Export};
    use yk_core::model::ItemTag;
    use yk_core::Key;

    #[test]
    fn reads_what_a_publisher_hands_over() {
        let parsed = bibtex(
            r#"@article{vaswani2017,
  author = {Vaswani, Ashish and Shazeer, Noam},
  title = {Attention Is All You Need},
  journal = {NeurIPS},
  year = {2017},
  pages = {5998--6008},
  doi = {10.5555/3295222},
}"#,
        );
        assert!(parsed.rejected.is_empty(), "{:?}", parsed.rejected);
        let item = &parsed.items[0];
        assert_eq!(item.item_type, "journalArticle");
        assert_eq!(item.fields["title"], "Attention Is All You Need");
        assert_eq!(item.fields["pages"], "5998-6008");
        assert_eq!(item.creators.len(), 2);
        assert_eq!(item.creators[0].last_name.as_deref(), Some("Vaswani"));
        assert_eq!(item.creators[1].first_name.as_deref(), Some("Noam"));
    }

    #[test]
    fn everything_this_writes_it_can_read_back() {
        // The property that matters, and the one that catches an escaping bug
        // in either direction: a title full of the characters that break
        // BibTeX survives a round trip through the file.
        let original = ItemDraft::new("journalArticle")
            .with_field("title", "100% of {Braces} & other_things")
            .with_field("publicationTitle", "Journal of Awkward Titles")
            .with_field("date", "2019")
            .with_field("pages", "10-20")
            .with_field("DOI", "10.1/x")
            .with_creator(Creator {
                creator_type: "author".into(),
                last_name: Some("Zhang".into()),
                first_name: Some("Wei".into()),
                name: None,
            })
            .into_item(Key::generate(), 1, 1);

        for format in [Export::BibTeX, Export::Ris] {
            let text = export(std::slice::from_ref(&original), format);
            let back = parse(&text);
            assert!(back.rejected.is_empty(), "{format:?}: {:?}", back.rejected);
            let item = &back.items[0];
            assert_eq!(item.fields["title"], "100% of {Braces} & other_things", "{format:?}");
            assert_eq!(item.fields["pages"], "10-20", "{format:?}");
            assert_eq!(item.fields["DOI"], "10.1/x", "{format:?}");
            assert_eq!(item.creators[0].last_name.as_deref(), Some("Zhang"), "{format:?}");
            assert_eq!(item.creators[0].first_name.as_deref(), Some("Wei"), "{format:?}");
        }
    }

    #[test]
    fn an_institution_survives_a_round_trip_whole() {
        let original = ItemDraft::new("report")
            .with_field("title", "Annual Report")
            .with_creator(Creator {
                creator_type: "author".into(),
                name: Some("World Health Organization".into()),
                last_name: None,
                first_name: None,
            })
            .into_item(Key::generate(), 1, 1);

        for format in [Export::BibTeX, Export::Ris] {
            let back = parse(&export(std::slice::from_ref(&original), format));
            let who = &back.items[0].creators[0];
            assert_eq!(who.name.as_deref(), Some("World Health Organization"), "{format:?}");
            assert!(who.last_name.is_none(), "{format:?}: split into a surname");
        }
    }

    #[test]
    fn tags_survive_a_round_trip() {
        let mut original = ItemDraft::new("journalArticle")
            .with_field("title", "Tagged")
            .into_item(Key::generate(), 1, 1);
        original.tags = vec![ItemTag::manual("transformers"), ItemTag::manual("nlp")];

        for format in [Export::BibTeX, Export::Ris] {
            let back = parse(&export(std::slice::from_ref(&original), format));
            let tags: Vec<&str> = back.items[0].tags.iter().map(|t| t.tag.as_str()).collect();
            assert_eq!(tags, ["transformers", "nlp"], "{format:?}");
        }
    }

    #[test]
    fn an_at_sign_inside_a_field_does_not_cut_the_entry_short() {
        // Splitting on `@` is the obvious way to find entries and it truncates
        // any record with an email address or a Twitter handle in it.
        let parsed = bibtex(
            r#"@misc{a, title = {Ask @someone about it}, author = {Li, Hua}, url = {mailto:a@b.c} }"#,
        );
        assert!(parsed.rejected.is_empty(), "{:?}", parsed.rejected);
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].fields["title"], "Ask @someone about it");
        assert_eq!(parsed.items[0].fields["url"], "mailto:a@b.c");
    }

    #[test]
    fn a_comma_inside_braces_is_not_a_field_boundary() {
        let parsed =
            bibtex(r#"@book{b, title = {Cities: London, Paris, and Rome}, publisher = {X} }"#);
        assert_eq!(parsed.items[0].fields["title"], "Cities: London, Paris, and Rome");
        assert_eq!(parsed.items[0].fields["publisher"], "X");
    }

    #[test]
    fn one_bad_record_does_not_cost_the_file() {
        // Forty references and the eleventh is malformed: refusing all forty
        // helps nobody.
        let parsed = bibtex(
            r#"@article{good1, title = {First}, author = {A, B} }
@comment{ this is not a reference }
@article{good2, title = {Second}, author = {C, D} }"#,
        );
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.rejected.len(), 1);
        assert_eq!(parsed.rejected[0].index, 2, "counted as records, not lines");
    }

    #[test]
    fn ris_records_that_never_say_er_are_still_read() {
        let parsed = ris("TY  - JOUR\nTI  - First\nTY  - BOOK\nTI  - Second\n");
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].fields["title"], "First");
        assert_eq!(parsed.items[1].item_type, "book");
    }

    #[test]
    fn a_wrapped_abstract_keeps_its_second_half() {
        let parsed = ris("TY  - JOUR\nTI  - X\nAB  - The first part\n  and the rest of it\nER  - \n");
        assert_eq!(parsed.items[0].fields["abstractNote"], "The first part and the rest of it");
    }

    #[test]
    fn the_format_is_recognised_from_the_file_rather_than_its_name() {
        // Downloaded as `citation.txt`, which says nothing.
        assert_eq!(parse("TY  - JOUR\nTI  - X\nER  - \n").items[0].item_type, "journalArticle");
        assert_eq!(parse("@book{k, title={X}}").items[0].item_type, "book");
    }

    #[test]
    fn a_record_with_nothing_in_it_is_refused_rather_than_imported() {
        // Otherwise the import leaves blank rows somebody has to hunt down.
        let parsed = bibtex("@article{empty, year = {2019} }");
        assert!(parsed.items.is_empty());
        assert_eq!(parsed.rejected.len(), 1);
    }

    /// The month has to come back, or the round trip loses it.
    ///
    /// BibTeX keeps year and month apart and the library stores one date, so
    /// `month` was read and dropped: exporting a March paper and importing it
    /// again returned a bare 2021.
    #[test]
    fn a_bibtex_month_rejoins_the_year() {
        for (written, expected) in
            [("mar", "2021-03"), ("{mar}", "2021-03"), ("3", "2021-03"), ("March", "2021-03")]
        {
            let text = format!("@article{{k, title = {{T}}, year = {{2021}}, month = {written},}}");
            let drafts = parse(&text).items;
            assert_eq!(
                drafts[0].fields.get("date").and_then(|d| d.as_str()),
                Some(expected),
                "month written as {written}",
            );
        }
    }

    /// A month nobody can read leaves the year alone rather than guessing.
    #[test]
    fn an_unreadable_month_is_ignored() {
        let text = "@article{k, title = {T}, year = {2021}, month = {spring},}";
        let drafts = parse(text).items;
        assert_eq!(drafts[0].fields.get("date").and_then(|d| d.as_str()), Some("2021"));
    }
}
