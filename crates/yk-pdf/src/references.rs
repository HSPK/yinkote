//! Reading a paper's reference list out of its own text.
//!
//! The last resort, and for a great deal of the literature the only one.
//! Crossref answers for a published article whose publisher deposited its
//! bibliography; Semantic Scholar answers for most preprints. What is left —
//! a thesis, a report, a scan somebody was handed, a paper whose publisher
//! deposited nothing — has its references written on the page and nowhere
//! else.
//!
//! This is deliberately conservative. A wrong reference is worse than a
//! missing one: it names a paper the author never cited, and there is no way
//! for a reader to tell it apart from a real one afterwards. So entries are
//! only taken from a recognised reference *section*, only when they look like
//! numbered or authored entries, and a DOI is only claimed when one is
//! actually printed.

use crate::Extracted;

/// One entry as the page has it: the raw line, and anything certain in it.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintedReference {
    /// The citation as printed, which is the label when nothing else is known.
    pub text: String,
    /// Only when a DOI is printed in the entry.
    pub doi: Option<String>,
    /// A four-digit year, when the entry carries exactly one plausible one.
    pub year: Option<i64>,
}

/// Headings that begin a reference list, lowercased.
///
/// Both spellings and the two languages this workbench is used in. "Works
/// cited" and "Literature cited" appear in the humanities and in biology
/// respectively.
const HEADINGS: [&str; 7] = [
    "references",
    "reference",
    "bibliography",
    "works cited",
    "literature cited",
    "参考文献",
    "引用文献",
];

/// Headings that end one, because a reference list is followed by appendices
/// as often as by the end of the file.
const ENDINGS: [&str; 6] = [
    "appendix",
    "appendices",
    "supplementary",
    "supporting information",
    "acknowledgements",
    "附录",
];

/// The references printed in a paper, or nothing when none can be read safely.
pub fn references(extracted: &Extracted) -> Vec<PrintedReference> {
    let Some(section) = reference_section(&extracted.text) else {
        return Vec::new();
    };
    let entries = split_entries(section);

    entries
        .into_iter()
        .filter(|e| plausible(e))
        .map(|text| PrintedReference {
            doi: doi_in(&text),
            year: year_in(&text),
            text,
        })
        .collect()
}

/// The text between a reference heading and whatever ends it.
///
/// The *last* heading, because a paper's introduction may say "see the
/// references in..." and its own list is the one at the end.
fn reference_section(text: &str) -> Option<&str> {
    let lower = text.to_lowercase();
    let start = HEADINGS
        .iter()
        .filter_map(|h| heading_at(&lower, h))
        .max()?;

    let rest = &text[start..];
    let rest_lower = &lower[start..];
    let end = ENDINGS
        .iter()
        .filter_map(|h| ending_at(rest_lower, h))
        .min()
        .unwrap_or(rest.len());

    Some(&rest[..end])
}

/// Where a heading stands on a line of its own, if it does.
///
/// On its own line, because "references" appears in ordinary sentences —
/// "our references to prior work" — and taking the middle of a paragraph as
/// the start of a bibliography would turn prose into citations.
fn heading_at(lower: &str, heading: &str) -> Option<usize> {
    let mut offset = 0;
    let mut found = None;
    for line in lower.split_inclusive('\n') {
        let trimmed = line.trim().trim_start_matches(|c: char| {
            c.is_ascii_digit() || c == '.' || c == ' ' || c == '\u{a0}'
        });
        let trimmed = trimmed.trim_end_matches(|c: char| c == ':' || c == '.' || c.is_whitespace());
        if trimmed == heading {
            found = Some(offset + line.len());
        }
        offset += line.len();
    }
    found
}

/// Where a section that ends the bibliography begins.
///
/// Looser than `heading_at` because these headings are numbered on the page —
/// "Appendix A", "Appendix 1" — while "References" stands alone. The suffix is
/// kept short so a sentence beginning "Appendix A describes..." in a paragraph
/// is not mistaken for the heading itself.
fn ending_at(lower: &str, heading: &str) -> Option<usize> {
    let mut offset = 0;
    for line in lower.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(heading) {
            let rest = rest.trim_start_matches([':', '.', ' ']);
            if rest.chars().count() <= 3 {
                return Some(offset);
            }
        }
        offset += line.len();
    }
    None
}

/// Break a reference section into entries.
///
/// Numbered lists (`[1]`, `1.`) are split on their markers; everything else
/// falls back to blank-line separation, which is how an unnumbered list is
/// laid out. A wrapped line inside one entry is joined back on, since a
/// bibliography is set to a narrow measure and almost every entry wraps.
fn split_entries(section: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.trim().is_empty() {
                entries.push(current.trim().to_string());
                current.clear();
            }
            continue;
        }
        if starts_entry(trimmed) && !current.trim().is_empty() {
            entries.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(trimmed);
    }
    if !current.trim().is_empty() {
        entries.push(current.trim().to_string());
    }
    entries
}

/// Whether a line opens a new entry: `[12]`, `12.`, or `12 `.
fn starts_entry(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.first() == Some(&b'[') {
        return line[1..].split(']').next().is_some_and(|n| {
            !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
        });
    }
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 3 {
        return false;
    }
    let after = line[digits.len()..].trim_start();
    // "12." or "12 Smith"; not "2020 was a good year", which has no marker.
    line[digits.len()..].starts_with('.') || after.starts_with(|c: char| c.is_uppercase())
}

/// Whether an entry is worth keeping.
///
/// Long enough to be a citation rather than a page number or a running head,
/// and carrying at least one thing a citation always has: a year, a DOI, or a
/// capitalised name followed by a comma.
fn plausible(entry: &str) -> bool {
    let text = entry.trim();
    if text.len() < 25 || text.len() > 600 {
        return false;
    }
    year_in(text).is_some() || doi_in(text).is_some()
}

/// A DOI printed in the entry, normalised.
fn doi_in(entry: &str) -> Option<String> {
    // Searched in the entry itself rather than in a lowercased copy: lowering
    // can change a string's length, so an offset found in one is not an offset
    // into the other. A DOI prefix is digits and a dot, which have no case.
    let at = entry.find("10.")?;
    let rest = &entry[at..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == ')' || c == ']')
        .unwrap_or(rest.len());
    let doi = rest[..end].trim_end_matches(['.', ',', ';']).to_lowercase();
    // `10.` followed by a registrant and a slash; anything else is a version
    // number or a measurement that happens to start the same way.
    let (prefix, suffix) = doi.split_once('/')?;
    let registrant = prefix.strip_prefix("10.")?;
    if registrant.len() < 4 || !registrant.chars().all(|c| c.is_ascii_digit()) || suffix.is_empty() {
        return None;
    }
    Some(doi)
}

/// A publication year, when the entry carries exactly one plausible one.
///
/// Exactly one: an entry naming two years — a reprint, or a page range that
/// looks like a year — cannot be assigned one without guessing.
fn year_in(entry: &str) -> Option<i64> {
    // Over bytes, not characters. Slicing `&entry[i..i + 4]` at a character
    // offset panics the moment a reference contains a multibyte character —
    // and this module reads 参考文献 as a heading, so that is not a corner
    // case, it is the second language it was written for. Four ASCII digits
    // are always four bytes, so a run of them is always a valid string.
    let mut found: Option<i64> = None;
    let bytes = entry.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if !bytes[i..i + 4].iter().all(u8::is_ascii_digit) {
            i += 1;
            continue;
        }
        // Not part of a longer number: 12345 is not a year.
        let before = i.checked_sub(1).map(|b| bytes[b]);
        let after = bytes.get(i + 4);
        if before.is_some_and(|b| b.is_ascii_digit()) || after.is_some_and(u8::is_ascii_digit) {
            i += 1;
            continue;
        }
        let year: i64 = std::str::from_utf8(&bytes[i..i + 4]).ok()?.parse().ok()?;
        if (1800..=2100).contains(&year) {
            if found.is_some_and(|f| f != year) {
                return None;
            }
            found = Some(year);
        }
        i += 4;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Vec<PrintedReference> {
        references(&Extracted { total_chars: text.len(), text: text.into(), truncated: false })
    }

    #[test]
    fn reads_a_numbered_list() {
        let found = read(
            "We build on prior work.\n\
             \n\
             References\n\
             [1] A. Vaswani et al. Attention is all you need. NeurIPS, 2017.\n\
             [2] J. Devlin et al. BERT: pre-training of deep bidirectional\n\
             transformers. NAACL, 2019. doi:10.18653/v1/N19-1423\n",
        );

        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].text.contains("Attention is all you need"));
        assert_eq!(found[0].year, Some(2017));
        // The wrapped second line is part of the entry, not an entry of its own.
        assert!(found[1].text.contains("transformers"), "a wrapped line was split off");
        assert_eq!(found[1].doi.as_deref(), Some("10.18653/v1/n19-1423"));
    }

    #[test]
    fn stops_at_the_appendix() {
        let found = read(
            "References\n\
             [1] A. Author. A real paper. Journal, 2011.\n\
             \n\
             Appendix A\n\
             [2] This is a numbered equation, not a citation, from 2011.\n",
        );
        assert_eq!(found.len(), 1, "read past the end of the list: {found:#?}");
    }

    /// The section is the *last* heading: papers refer to references in prose.
    #[test]
    fn is_not_fooled_by_the_word_in_a_sentence() {
        let found = read(
            "Our references to prior work are extensive and we thank the\n\
             reviewers for pointing us at further references in the field.\n\
             \n\
             References\n\
             [1] A. Author. The only real entry here. Journal, 2011.\n",
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].text.contains("The only real entry"));
    }

    /// Nothing at all is the right answer for a paper whose list cannot be
    /// found. A wrong reference names a paper the author never cited, and
    /// nothing downstream can tell it from a real one.
    #[test]
    fn says_nothing_rather_than_guessing() {
        assert!(read("A paper with no reference section at all. 2019.").is_empty());
        assert!(
            read("References\nsee our website\n").is_empty(),
            "kept an entry with neither a year nor an identifier"
        );
    }

    #[test]
    fn takes_a_doi_only_when_one_is_printed() {
        assert_eq!(doi_in("Smith, 2019. doi:10.1000/abc123."), Some("10.1000/abc123".into()));
        // Version numbers and measurements start the same way.
        assert_eq!(doi_in("we used version 10.2 of the toolkit"), None);
        assert_eq!(doi_in("10.5/x"), None, "a two-digit registrant is not a DOI");
    }

    /// This module reads 参考文献 as a heading, so a reference list full of
    /// non-ASCII text is the second case it was written for — and the first
    /// version panicked on it, taking a worker thread down, because the year
    /// scan sliced on byte offsets. Every fixture above is ASCII, which is why
    /// six passing tests said nothing about it.
    #[test]
    fn reads_a_list_that_is_not_ascii() {
        let found = read(
            "参考文献\n\
             [1] 张三, 李四. 中文期刊上的一篇论文. 心理学报, 2019.\n\
             [2] Müller, K. Über die Struktur. Zeitschrift für Physik, 2003.\n",
        );

        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].year, Some(2019));
        assert_eq!(found[1].year, Some(2003));
    }

    #[test]
    fn finds_a_doi_after_a_multibyte_character() {
        // The offset came from a lowercased copy, whose length can differ.
        assert_eq!(
            doi_in("Müller, K. Über alles. 2003. doi:10.1000/xyz"),
            Some("10.1000/xyz".into())
        );
    }

    #[test]
    fn refuses_a_year_it_cannot_choose() {
        assert_eq!(year_in("A paper. Journal, 2019."), Some(2019));
        // A reprint carries two, and picking one would be a guess.
        assert_eq!(year_in("Originally 1936, reprinted 2001."), None);
        assert_eq!(year_in("Pages 10234-10240."), None, "a long number is not a year");
    }
}
