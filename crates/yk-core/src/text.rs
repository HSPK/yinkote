//! Text normalisation helpers shared by storage, search and dedup.

/// Lowercase, strip accents-ish, collapse whitespace and drop punctuation.
/// Deliberately simple and allocation-light; good enough for fingerprints.
pub fn normalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = true;
    for ch in input.chars() {
        let c = ch.to_lowercase().next().unwrap_or(ch);
        if c.is_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Tokenise for indexing. CJK characters become individual tokens *plus*
/// bigrams, which gives usable recall without shipping a dictionary.
pub fn tokenize(input: &str) -> Vec<String> {
    tokenize_inner(input, true)
}

/// Tokenise for querying.
///
/// Identical to [`tokenize`] except that a run of two or more CJK characters
/// contributes only its bigrams. The unigrams are redundant — every document
/// containing the bigrams contains the characters — and each extra ANDed term
/// costs a postings-list merge. Dropping them measured 1.5x faster with
/// unchanged results. Single characters still emit a unigram so one-character
/// queries keep working.
pub fn tokenize_query(input: &str) -> Vec<String> {
    tokenize_inner(input, false)
}

fn tokenize_inner(input: &str, unigrams: bool) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut latin = String::new();
    let mut cjk: Vec<char> = Vec::new();

    let flush_latin = |latin: &mut String, tokens: &mut Vec<String>| {
        if !latin.is_empty() {
            tokens.push(std::mem::take(latin));
        }
    };

    for ch in input.chars() {
        if is_cjk(ch) {
            flush_latin(&mut latin, &mut tokens);
            cjk.push(ch);
        } else if ch.is_alphanumeric() {
            latin.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            flush_latin(&mut latin, &mut tokens);
        }
    }
    flush_latin(&mut latin, &mut tokens);

    let emit_unigrams = unigrams || cjk.len() < 2;
    for (i, c) in cjk.iter().enumerate() {
        if emit_unigrams {
            tokens.push(c.to_string());
        }
        if i + 1 < cjk.len() {
            tokens.push(format!("{}{}", c, cjk[i + 1]));
        }
    }
    tokens
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // kana
        | 0x3400..=0x4DBF // CJK ext A
        | 0x4E00..=0x9FFF // CJK unified
        | 0xF900..=0xFAFF
        | 0xAC00..=0xD7AF // hangul
    )
}

pub fn contains_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

/// Character trigrams of the normalised string, used for fuzzy matching.
pub fn trigrams(input: &str) -> Vec<String> {
    let norm = normalize(input);
    let chars: Vec<char> = format!("  {norm} ").chars().collect();
    if chars.len() < 3 {
        return vec![norm];
    }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

/// Normalised Levenshtein similarity in `[0,1]`.
pub fn similarity(a: &str, b: &str) -> f32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let max = a.len().max(b.len());
    if max == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(&a, &b) as f32 / max as f32)
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises() {
        assert_eq!(normalize("  Hello, World!  "), "hello world");
    }

    #[test]
    fn tokenises_mixed_scripts() {
        let t = tokenize("Diffusion 扩散模型");
        assert!(t.contains(&"diffusion".to_string()));
        assert!(t.contains(&"扩".to_string()));
        assert!(t.contains(&"扩散".to_string()));
    }

    #[test]
    fn query_tokens_drop_redundant_cjk_unigrams() {
        let q = tokenize_query("扩散模型");
        assert_eq!(q, vec!["扩散", "散模", "模型"]);
        // Indexing keeps both so single-character queries still match.
        assert!(tokenize("扩散模型").contains(&"扩".to_string()));
    }

    #[test]
    fn single_cjk_character_still_queryable() {
        assert_eq!(tokenize_query("扩"), vec!["扩"]);
    }

    #[test]
    fn query_tokens_keep_latin_unchanged() {
        assert_eq!(tokenize_query("Diffusion 扩散模型"), vec!["diffusion", "扩散", "散模", "模型"]);
    }

    #[test]
    fn similarity_is_sane() {
        assert!(similarity("attention", "attention") > 0.99);
        assert!(similarity("attention", "attension") > 0.8);
        assert!(similarity("attention", "banana") < 0.4);
    }
}
