use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Alphabet without visually ambiguous characters (no I, L, O, U, 0, 1).
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";
const KEY_LEN: usize = 8;

/// A stable, opaque, cross-device identifier for a domain object.
///
/// Keys are what the outside world sees. Numeric row ids never leave the
/// storage layer, which keeps the API stable across imports and merges.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Key(String);

impl Key {
    pub fn generate() -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let s: String = (0..KEY_LEN)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect();
        Key(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// Accepts an existing key (e.g. imported from Zotero) after validation.
    pub fn parse(s: &str) -> crate::Result<Self> {
        let s = s.trim();
        if s.is_empty() || s.len() > 32 {
            return Err(crate::Error::invalid(format!("bad key {s:?}")));
        }
        if !s.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(crate::Error::invalid(format!("bad key {s:?}")));
        }
        Ok(Key(s.to_ascii_uppercase()))
    }
}

impl FromStr for Key {
    type Err = crate::Error;
    fn from_str(s: &str) -> crate::Result<Self> {
        Key::parse(s)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({})", self.0)
    }
}

impl From<Key> for String {
    fn from(k: Key) -> String {
        k.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_well_formed() {
        let k = Key::generate();
        assert_eq!(k.as_str().len(), KEY_LEN);
        assert!(Key::parse(k.as_str()).is_ok());
    }

    #[test]
    fn rejects_malformed_keys() {
        assert!(Key::parse("").is_err());
        assert!(Key::parse("with space").is_err());
        assert!(Key::parse("A".repeat(40).as_str()).is_err());
    }

    #[test]
    fn normalises_case() {
        assert_eq!(Key::parse("abcd1234").unwrap().as_str(), "ABCD1234");
    }
}
