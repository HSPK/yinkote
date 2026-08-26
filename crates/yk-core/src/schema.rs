//! Schema-driven item types.
//!
//! The single source of truth is `assets/item-types.json`, embedded at compile
//! time and shared with the frontend over `GET /api/v1/schema`. Adding an item
//! type or field never requires a migration or a code change.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::Fields;
use crate::{Error, Result};

const RAW: &str = include_str!("../assets/item-types.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FieldDef {
    pub r#type: String,
    pub label: String,
    #[serde(rename = "labelEn")]
    pub label_en: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ItemTypeDef {
    pub r#type: String,
    pub label: String,
    #[serde(rename = "labelEn")]
    pub label_en: String,
    pub csl: String,
    pub fields: Vec<String>,
    #[serde(rename = "creatorTypes")]
    pub creator_types: Vec<String>,
    /// Not offered in the "new item" menu (note / attachment).
    #[serde(default)]
    pub internal: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Schema {
    pub version: u32,
    #[serde(rename = "baseFields")]
    pub base_fields: Vec<String>,
    pub fields: HashMap<String, FieldDef>,
    #[serde(rename = "itemTypes")]
    pub item_types: Vec<ItemTypeDef>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl Schema {
    pub fn get(&self, item_type: &str) -> Option<&ItemTypeDef> {
        self.index.get(item_type).map(|i| &self.item_types[*i])
    }

    pub fn has_type(&self, item_type: &str) -> bool {
        self.index.contains_key(item_type)
    }

    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields.get(name)
    }

    /// Validate a draft against its declared type.
    ///
    /// Unknown fields are *kept*, not rejected — round-tripping exotic formats
    /// matters more than strictness — but they are reported so callers can warn.
    pub fn validate(&self, item_type: &str, fields: &Fields) -> Result<Vec<String>> {
        let def = self
            .get(item_type)
            .ok_or_else(|| Error::invalid(format!("unknown itemType '{item_type}'")))?;
        let mut unknown = Vec::new();
        for key in fields.keys() {
            if !def.fields.iter().any(|f| f == key) && !self.fields.contains_key(key) {
                unknown.push(key.clone());
            }
        }
        Ok(unknown)
    }

    pub fn validate_creator_type(&self, item_type: &str, creator_type: &str) -> bool {
        self.get(item_type)
            .map(|d| d.creator_types.iter().any(|c| c == creator_type))
            .unwrap_or(false)
    }

    /// Item types offered to users when creating an item.
    pub fn public_types(&self) -> impl Iterator<Item = &ItemTypeDef> {
        self.item_types.iter().filter(|t| !t.internal)
    }
}

static SCHEMA: OnceLock<Schema> = OnceLock::new();

/// The process-wide schema. Panics only if the embedded asset is corrupt,
/// which is a build-time guarantee covered by a test.
pub fn schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let mut s: Schema = serde_json::from_str(RAW).expect("embedded item-types.json is valid");
        s.index = s
            .item_types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.r#type.clone(), i))
            .collect();
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_schema_parses() {
        let s = schema();
        assert!(s.item_types.len() > 10);
        assert!(s.has_type("journalArticle"));
        assert_eq!(s.get("journalArticle").unwrap().csl, "article-journal");
    }

    #[test]
    fn every_declared_field_has_a_definition() {
        let s = schema();
        for t in &s.item_types {
            for f in &t.fields {
                assert!(s.field(f).is_some(), "field '{f}' of '{}' undefined", t.r#type);
            }
        }
    }

    #[test]
    fn validate_reports_unknown_fields() {
        let s = schema();
        let mut fields = Fields::new();
        fields.insert("title".into(), "x".into());
        fields.insert("totallyMadeUp".into(), "y".into());
        let unknown = s.validate("journalArticle", &fields).unwrap();
        assert_eq!(unknown, vec!["totallyMadeUp".to_string()]);
        assert!(s.validate("nope", &fields).is_err());
    }
}
