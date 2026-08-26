//! Search query language.
//!
//! Users type one box; power users get field operators. Anything not matched by
//! an operator falls through as free text.
//!
//! ```text
//! diffusion tag:综述 -tag:obsolete type:book year:2020..2024 author:zhang "exact phrase"
//! ```

use yk_core::query::ItemFilter;

#[derive(Debug, Default, PartialEq)]
pub struct ParsedQuery {
    /// Free text, operators stripped.
    pub text: String,
    /// Quoted phrases that must appear verbatim.
    pub phrases: Vec<String>,
    /// Tag names; a leading `-` means "exclude".
    pub tags: Vec<String>,
    pub item_types: Vec<String>,
    pub creators: Vec<String>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
}

impl ParsedQuery {
    pub fn parse(input: &str) -> Self {
        let mut q = ParsedQuery::default();
        let mut free: Vec<String> = Vec::new();

        for token in lex(input) {
            match token {
                Token::Phrase(p) => {
                    free.push(p.clone());
                    q.phrases.push(p);
                }
                Token::Word(w) => free.push(w),
                Token::Field { negated, name, value } => match name.as_str() {
                    "tag" | "标签" => {
                        q.tags.push(if negated { format!("-{value}") } else { value })
                    }
                    "type" | "类型" => q.item_types.push(value),
                    "author" | "creator" | "作者" => q.creators.push(value),
                    "year" | "年" => apply_year(&mut q, &value),
                    // Unknown operator: treat the whole thing as text rather
                    // than silently dropping the user's input.
                    _ => free.push(format!("{name}:{value}")),
                },
            }
        }
        q.text = free.join(" ");
        q
    }

    /// Whether anything other than free text was specified.
    pub fn has_constraints(&self) -> bool {
        !self.tags.is_empty()
            || !self.item_types.is_empty()
            || !self.creators.is_empty()
            || self.year_from.is_some()
            || self.year_to.is_some()
    }

    /// Fold the parsed constraints into an existing filter.
    pub fn apply_to(&self, filter: &mut ItemFilter) {
        filter.tags.extend(self.tags.iter().cloned());
        filter.item_types.extend(self.item_types.iter().cloned());
    }
}

fn apply_year(q: &mut ParsedQuery, value: &str) {
    if let Some((a, b)) = value.split_once("..") {
        q.year_from = a.parse().ok();
        q.year_to = b.parse().ok();
    } else if let Some(rest) = value.strip_prefix(">=") {
        q.year_from = rest.parse().ok();
    } else if let Some(rest) = value.strip_prefix("<=") {
        q.year_to = rest.parse().ok();
    } else if let Some(rest) = value.strip_prefix('>') {
        q.year_from = rest.parse::<i32>().ok().map(|y| y + 1);
    } else if let Some(rest) = value.strip_prefix('<') {
        q.year_to = rest.parse::<i32>().ok().map(|y| y - 1);
    } else if let Ok(y) = value.parse::<i32>() {
        q.year_from = Some(y);
        q.year_to = Some(y);
    }
}

#[derive(Debug)]
enum Token {
    Word(String),
    Phrase(String),
    Field { negated: bool, name: String, value: String },
}

/// Split on whitespace while honouring double quotes.
fn lex(input: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '"' {
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && chars[j] != '"' {
                j += 1;
            }
            let phrase: String = chars[start..j.min(chars.len())].iter().collect();
            if !phrase.is_empty() {
                out.push(Token::Phrase(phrase));
            }
            i = j + 1;
            continue;
        }

        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let raw: String = chars[start..i].iter().collect();
        out.push(classify(&raw));
    }
    out
}

fn classify(raw: &str) -> Token {
    let (negated, body) = match raw.strip_prefix('-') {
        Some(rest) if rest.contains(':') => (true, rest),
        _ => (false, raw),
    };
    match body.split_once(':') {
        Some((name, value)) if !name.is_empty() && !value.is_empty() => Token::Field {
            negated,
            name: name.to_ascii_lowercase(),
            value: value.trim_matches('"').to_string(),
        },
        _ => Token::Word(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        let q = ParsedQuery::parse("diffusion models");
        assert_eq!(q.text, "diffusion models");
        assert!(!q.has_constraints());
    }

    #[test]
    fn extracts_tags_including_negated() {
        let q = ParsedQuery::parse("llm tag:survey -tag:obsolete");
        assert_eq!(q.text, "llm");
        assert_eq!(q.tags, vec!["survey", "-obsolete"]);
    }

    #[test]
    fn extracts_type_and_author() {
        let q = ParsedQuery::parse("type:book author:zhang neural");
        assert_eq!(q.item_types, vec!["book"]);
        assert_eq!(q.creators, vec!["zhang"]);
        assert_eq!(q.text, "neural");
    }

    #[test]
    fn parses_year_forms() {
        assert_eq!(ParsedQuery::parse("year:2020..2024").year_from, Some(2020));
        assert_eq!(ParsedQuery::parse("year:2020..2024").year_to, Some(2024));
        assert_eq!(ParsedQuery::parse("year:>2019").year_from, Some(2020));
        assert_eq!(ParsedQuery::parse("year:<=2019").year_to, Some(2019));
        let exact = ParsedQuery::parse("year:2021");
        assert_eq!((exact.year_from, exact.year_to), (Some(2021), Some(2021)));
    }

    #[test]
    fn keeps_quoted_phrases() {
        let q = ParsedQuery::parse(r#"attention "is all you need""#);
        assert_eq!(q.phrases, vec!["is all you need"]);
        assert_eq!(q.text, "attention is all you need");
    }

    #[test]
    fn unknown_operator_is_kept_as_text() {
        let q = ParsedQuery::parse("wat:ever");
        assert_eq!(q.text, "wat:ever");
    }

    #[test]
    fn chinese_operators_work() {
        let q = ParsedQuery::parse("扩散 标签:综述 类型:book");
        assert_eq!(q.tags, vec!["综述"]);
        assert_eq!(q.item_types, vec!["book"]);
        assert_eq!(q.text, "扩散");
    }
}
