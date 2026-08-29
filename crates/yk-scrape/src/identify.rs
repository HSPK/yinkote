//! Identifier detection.
//!
//! Users paste whatever they have: a bare DOI, a publisher URL, a full citation
//! copied from a PDF, or a line from a reference list. This module finds every
//! usable identifier in that text and ranks them, so the caller can resolve the
//! most specific one first.
//!
//! Everything here is pure and offline — the network only enters at resolution
//! time — which keeps it exhaustively testable.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Identifier {
    /// Bare DOI, lower-cased, no `https://doi.org/` prefix.
    Doi(String),
    /// arXiv id including any version suffix, e.g. `2101.12345v2`.
    ArXiv(String),
    Pmid(String),
    Pmcid(String),
    /// Normalised to digits (plus a trailing `X` for ISBN-10).
    Isbn(String),
    Url(String),
}

impl Identifier {
    pub fn kind(&self) -> &'static str {
        match self {
            Identifier::Doi(_) => "doi",
            Identifier::ArXiv(_) => "arxiv",
            Identifier::Pmid(_) => "pmid",
            Identifier::Pmcid(_) => "pmcid",
            Identifier::Isbn(_) => "isbn",
            Identifier::Url(_) => "url",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Identifier::Doi(v)
            | Identifier::ArXiv(v)
            | Identifier::Pmid(v)
            | Identifier::Pmcid(v)
            | Identifier::Isbn(v)
            | Identifier::Url(v) => v,
        }
    }

    /// Lower is better. A DOI names the work itself; a URL only names a page
    /// that happens to be about it, so it is the last resort.
    fn specificity(&self) -> u8 {
        match self {
            Identifier::Doi(_) => 0,
            Identifier::ArXiv(_) => 1,
            Identifier::Pmcid(_) => 2,
            Identifier::Pmid(_) => 3,
            Identifier::Isbn(_) => 4,
            Identifier::Url(_) => 5,
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind(), self.value())
    }
}

/// Find every identifier in `text`, most specific first, without duplicates.
pub fn detect(text: &str) -> Vec<Identifier> {
    let mut found: Vec<Identifier> = Vec::new();
    let mut push = |id: Identifier| {
        if !found.contains(&id) {
            found.push(id);
        }
    };

    for id in dois(text) {
        push(Identifier::Doi(id));
    }
    for id in arxiv_ids(text) {
        push(Identifier::ArXiv(id));
    }
    for id in pmcids(text) {
        push(Identifier::Pmcid(id));
    }
    for id in pmids(text) {
        push(Identifier::Pmid(id));
    }
    for id in isbns(text) {
        push(Identifier::Isbn(id));
    }
    for url in urls(text) {
        push(Identifier::Url(url));
    }

    found.sort_by_key(|i| i.specificity());
    found
}

/// The single best identifier in `text`, if any.
pub fn detect_one(text: &str) -> Option<Identifier> {
    detect(text).into_iter().next()
}

// ---------------------------------------------------------------------------
// Scanners
//
// Hand-rolled rather than regex: the crate stays dependency-free, and each
// scanner can apply the trailing-punctuation and checksum rules that a naive
// pattern would get wrong.
// ---------------------------------------------------------------------------

/// Characters a DOI may contain after the `10.xxxx/` prefix.
fn is_doi_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._;()/:<>+[]".contains(c)
}

/// Publishers habitually append punctuation to a DOI in running text.
/// Unbalanced brackets are stripped too, since `(doi:10.1/x)` is common.
fn trim_doi(raw: &str) -> &str {
    let mut end = raw.len();
    while end > 0 {
        let candidate = &raw[..end];
        let last = candidate.chars().last().unwrap();
        let unbalanced = match last {
            ')' => candidate.matches('(').count() < candidate.matches(')').count(),
            ']' => candidate.matches('[').count() < candidate.matches(']').count(),
            '>' => candidate.matches('<').count() < candidate.matches('>').count(),
            _ => false,
        };
        if ".,;:'\"".contains(last) || unbalanced {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    &raw[..end]
}

fn dois(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while let Some(rel) = lower[i..].find("10.") {
        let start = i + rel;
        // The registrant code is 4-9 digits followed by `/`.
        let digits = lower[start + 3..].chars().take_while(char::is_ascii_digit).count();
        let slash = start + 3 + digits;
        if !(4..=9).contains(&digits) || bytes.get(slash) != Some(&b'/') {
            i = start + 3;
            continue;
        }
        let suffix_len = lower[slash + 1..].chars().take_while(|c| is_doi_char(*c)).count();
        if suffix_len == 0 {
            i = start + 3;
            continue;
        }
        let raw = &lower[start..slash + 1 + suffix_len];
        let doi = trim_doi(raw);
        if doi.len() > slash + 1 - start {
            out.push(doi.to_string());
        }
        i = start + raw.len();
    }
    out
}

fn arxiv_ids(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();

    // Modern form: 4 digits, dot, 4-5 digits, optional version.
    for marker in ["arxiv:", "arxiv.org/abs/", "arxiv.org/pdf/", "arxiv.org/html/"] {
        let mut i = 0;
        while let Some(rel) = lower[i..].find(marker) {
            let start = i + rel + marker.len();
            let token: String =
                lower[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || ".-/".contains(*c)).collect();
            let token = token.trim_end_matches(['.', '-']).trim_end_matches(".pdf");
            if let Some(id) = normalise_arxiv(token) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
            i = start;
        }
    }

    // Bare modern ids like `2101.12345` appearing on their own.
    if out.is_empty() {
        for token in lower.split(|c: char| c.is_whitespace() || c == ',') {
            let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
            if let Some(id) = normalise_arxiv(token) {
                if looks_like_bare_arxiv(token) && !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// `2101.12345` and `2101.12345v3` only — never a version-less decimal number.
fn looks_like_bare_arxiv(token: &str) -> bool {
    let (head, tail) = match token.split_once('.') {
        Some(parts) => parts,
        None => return false,
    };
    let digits = tail.chars().take_while(char::is_ascii_digit).count();
    head.len() == 4
        && head.chars().all(|c| c.is_ascii_digit())
        && (4..=5).contains(&digits)
        && tail[digits..].chars().all(|c| c == 'v' || c.is_ascii_digit())
}

fn normalise_arxiv(token: &str) -> Option<String> {
    let token = token.trim_matches('/');
    if token.is_empty() {
        return None;
    }
    // Old style: `math.GT/0309136`
    if let Some((subject, number)) = token.split_once('/') {
        let ok = subject.chars().all(|c| c.is_ascii_alphabetic() || c == '.' || c == '-')
            && number.len() >= 7
            && number.chars().take(7).all(|c| c.is_ascii_digit());
        return ok.then(|| token.to_string());
    }
    looks_like_bare_arxiv(token).then(|| token.to_string())
}

fn numeric_after(text: &str, markers: &[&str], min_len: usize, prefix: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for marker in markers {
        let mut i = 0;
        while let Some(rel) = lower[i..].find(marker) {
            let start = i + rel + marker.len();
            let rest = lower[start..].trim_start_matches([' ', ':', '=', '/']);
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if digits.len() >= min_len {
                let value = format!("{prefix}{digits}");
                if !out.contains(&value) {
                    out.push(value);
                }
            }
            i = start;
        }
    }
    out
}

fn pmids(text: &str) -> Vec<String> {
    numeric_after(text, &["pmid", "pubmed.ncbi.nlm.nih.gov/", "pubmed/"], 5, "")
}

fn pmcids(text: &str) -> Vec<String> {
    numeric_after(text, &["pmc"], 5, "PMC")
}

fn isbns(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let cleaned: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < cleaned.len() {
        if !cleaned[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        // Collect a run of digits, hyphens and spaces, plus a possible X.
        let mut digits = String::new();
        let mut j = i;
        while j < cleaned.len() && digits.len() < 13 {
            let c = cleaned[j];
            if c.is_ascii_digit() {
                digits.push(c);
            } else if c == '-' || c == ' ' {
                // separators are allowed inside, but not two in a row
                if j + 1 >= cleaned.len() || !(cleaned[j + 1].is_ascii_digit() || cleaned[j + 1] == 'X')
                {
                    break;
                }
            } else if (c == 'X' || c == 'x') && digits.len() == 9 {
                digits.push('X');
                j += 1;
                break;
            } else {
                break;
            }
            j += 1;
        }
        if (digits.len() == 10 || digits.len() == 13) && valid_isbn(&digits) {
            if !out.contains(&digits) {
                out.push(digits);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Checksum validation is what stops every 13-digit number from looking like a
/// book.
fn valid_isbn(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    match chars.len() {
        10 => {
            let mut sum = 0usize;
            for (i, c) in chars.iter().enumerate() {
                let digit = if *c == 'X' && i == 9 {
                    10
                } else {
                    match c.to_digit(10) {
                        Some(d) => d as usize,
                        None => return false,
                    }
                };
                sum += digit * (10 - i);
            }
            sum.is_multiple_of(11)
        }
        13 => {
            let mut sum = 0usize;
            for (i, c) in chars.iter().enumerate() {
                let Some(d) = c.to_digit(10) else { return false };
                sum += d as usize * if i.is_multiple_of(2) { 1 } else { 3 };
            }
            sum.is_multiple_of(10)
        }
        _ => false,
    }
}

/// Drop the punctuation that ended the sentence, not the punctuation that is
/// part of the address.
///
/// `(see https://example.com/paper)` must not keep its bracket, and
/// `https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)`
/// must keep both of its. Stripping unconditionally turned every Wikipedia
/// page with a disambiguator -- and every DOI with a bracket in it -- into a
/// 404, reported as "no record", which reads as the page not existing.
///
/// So a closing bracket is only sentence punctuation when it has no opener.
fn trim_trailing_punctuation(url: &str) -> &str {
    let mut end = url.len();
    while let Some(last) = url[..end].chars().last() {
        let counts = |open: char, close: char| {
            url[..end].matches(close).count() > url[..end].matches(open).count()
        };
        let sentence = match last {
            '.' | ',' | ';' => true,
            ')' => counts('(', ')'),
            ']' => counts('[', ']'),
            _ => false,
        };
        if !sentence {
            break;
        }
        end -= last.len_utf8();
    }
    &url[..end]
}

fn urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for scheme in ["https://", "http://"] {
        let mut i = 0;
        while let Some(rel) = text[i..].find(scheme) {
            let start = i + rel;
            let end = text[start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>')
                .map(|n| start + n)
                .unwrap_or(text.len());
            let url = trim_trailing_punctuation(&text[start..end]);
            if url.len() > scheme.len() && !out.contains(&url.to_string()) {
                out.push(url.to_string());
            }
            i = end.max(start + scheme.len());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(text: &str) -> Identifier {
        detect_one(text).unwrap_or_else(|| panic!("nothing detected in {text:?}"))
    }

    #[test]
    fn finds_a_bare_doi() {
        assert_eq!(only("10.1038/nature14539"), Identifier::Doi("10.1038/nature14539".into()));
    }

    #[test]
    fn finds_a_doi_inside_a_url() {
        assert_eq!(
            only("https://doi.org/10.1145/3292500.3330701"),
            Identifier::Doi("10.1145/3292500.3330701".into())
        );
    }

    #[test]
    fn finds_a_doi_inside_a_citation() {
        let citation = "Vaswani et al. (2017). Attention is all you need. \
                        NeurIPS. doi:10.48550/arXiv.1706.03762.";
        assert_eq!(only(citation), Identifier::Doi("10.48550/arxiv.1706.03762".into()));
    }

    #[test]
    fn strips_trailing_punctuation_from_dois() {
        assert_eq!(only("see 10.1000/xyz123."), Identifier::Doi("10.1000/xyz123".into()));
        assert_eq!(only("(10.1000/xyz123)"), Identifier::Doi("10.1000/xyz123".into()));
        assert_eq!(only("[10.1000/xyz123]"), Identifier::Doi("10.1000/xyz123".into()));
    }

    #[test]
    fn keeps_balanced_brackets_that_belong_to_the_doi() {
        assert_eq!(
            only("10.1002/(sici)1097-0258(19980815)17:15<1661::aid-sim968>3.0.co;2-2"),
            Identifier::Doi("10.1002/(sici)1097-0258(19980815)17:15<1661::aid-sim968>3.0.co;2-2".into())
        );
    }

    #[test]
    fn rejects_non_dois() {
        assert!(detect("10.5 metres").is_empty());
        assert!(detect("version 10.14/").is_empty());
    }

    #[test]
    fn finds_arxiv_in_every_common_shape() {
        for text in [
            "arXiv:2101.12345",
            "https://arxiv.org/abs/2101.12345",
            "https://arxiv.org/pdf/2101.12345.pdf",
            "2101.12345",
        ] {
            let ids = detect(text);
            assert!(
                ids.contains(&Identifier::ArXiv("2101.12345".into())),
                "{text:?} produced {ids:?}"
            );
        }
    }

    #[test]
    fn keeps_the_arxiv_version_suffix() {
        assert!(detect("arXiv:2101.12345v3").contains(&Identifier::ArXiv("2101.12345v3".into())));
    }

    #[test]
    fn understands_legacy_arxiv_ids() {
        assert!(detect("arXiv:math.GT/0309136")
            .contains(&Identifier::ArXiv("math.gt/0309136".into())));
    }

    #[test]
    fn does_not_mistake_decimals_for_arxiv_ids() {
        assert!(!detect("the value was 3.14159").iter().any(|i| matches!(i, Identifier::ArXiv(_))));
        assert!(!detect("2101.1").iter().any(|i| matches!(i, Identifier::ArXiv(_))));
    }

    #[test]
    fn finds_pubmed_identifiers() {
        assert!(detect("PMID: 26017442").contains(&Identifier::Pmid("26017442".into())));
        assert!(detect("https://pubmed.ncbi.nlm.nih.gov/26017442/")
            .contains(&Identifier::Pmid("26017442".into())));
        assert!(detect("PMC4404800").contains(&Identifier::Pmcid("PMC4404800".into())));
    }

    #[test]
    fn validates_isbn_checksums() {
        assert!(detect("9787111213826").contains(&Identifier::Isbn("9787111213826".into())));
        assert!(detect("ISBN 0-306-40615-2").contains(&Identifier::Isbn("0306406152".into())));
        // One digit changed: the checksum must reject it.
        assert!(!detect("9787111213827").iter().any(|i| matches!(i, Identifier::Isbn(_))));
    }

    #[test]
    fn accepts_isbn10_ending_in_x() {
        assert!(detect("ISBN 043942089X").contains(&Identifier::Isbn("043942089X".into())));
    }

    #[test]
    fn does_not_treat_a_year_or_phone_number_as_an_isbn() {
        assert!(!detect("published in 2017").iter().any(|i| matches!(i, Identifier::Isbn(_))));
    }

    #[test]
    fn finds_plain_urls() {
        assert_eq!(
            only("https://example.com/paper"),
            Identifier::Url("https://example.com/paper".into())
        );
    }

    /// A bracket in the address is part of the address.
    ///
    /// Wikipedia disambiguates with one -- `Transformer_(deep_learning_
    /// architecture)` -- and so do plenty of DOIs. Stripping it unconditionally
    /// asked the server for a URL one character short, which answered 404, and
    /// the reader was told "no record": the page not existing, rather than us
    /// having asked for the wrong one.
    #[test]
    fn a_bracket_inside_an_address_is_kept() {
        let wiki = "https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)";
        assert_eq!(only(wiki), Identifier::Url(wiki.into()));

        let doi = "https://doi.org/10.1002/(SICI)1097-0142(19970815)80:4<771::AID>3.0.CO;2-A";
        assert!(matches!(only(doi), Identifier::Doi(_) | Identifier::Url(_)));
    }

    /// And the other half, which is why the stripping exists: punctuation that
    /// ended the sentence is not part of the address.
    #[test]
    fn punctuation_that_ended_the_sentence_is_dropped() {
        assert_eq!(
            only("See https://example.com/paper."),
            Identifier::Url("https://example.com/paper".into())
        );
        assert_eq!(
            only("(see https://example.com/paper)"),
            Identifier::Url("https://example.com/paper".into()),
            "a closing bracket with no opener inside the url is the sentence's"
        );
        assert_eq!(
            only("Read https://example.com/paper, then stop"),
            Identifier::Url("https://example.com/paper".into())
        );
    }

    #[test]
    fn prefers_the_most_specific_identifier() {
        let text = "https://www.nature.com/articles/nature14539 doi:10.1038/nature14539";
        assert_eq!(only(text), Identifier::Doi("10.1038/nature14539".into()));
        // The URL is still available as a fallback.
        assert_eq!(detect(text).len(), 2);
    }

    #[test]
    fn deduplicates_repeated_identifiers() {
        let text = "10.1038/nature14539 and again 10.1038/nature14539";
        assert_eq!(detect(text).len(), 1);
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert!(detect("").is_empty());
        assert!(detect("just some prose with no identifiers").is_empty());
    }

    #[test]
    fn handles_multiline_paste() {
        let text = "First: 10.1000/aaa1\nSecond: arXiv:2101.12345\nThird: PMID 26017442";
        let ids = detect(text);
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0].kind(), "doi", "sorted most specific first");
    }
}
