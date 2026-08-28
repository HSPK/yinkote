use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{now_ms, text, Key};

/// Free-form, schema-validated field bag for an item (title, DOI, date, ...).
pub type Fields = Map<String, Value>;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Creator {
    #[serde(rename = "creatorType", default = "default_creator_type")]
    pub creator_type: String,
    /// Two-field mode.
    #[serde(rename = "firstName", skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(rename = "lastName", skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    /// Single-field mode (institutions, most CJK personal names).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn default_creator_type() -> String {
    "author".into()
}

impl Creator {
    /// A person whose name the source split for us.
    ///
    /// Two constructors rather than a builder because there are exactly two
    /// cases and every source is one of them: either it told us which part is
    /// the family name, or it handed over one string. Deciding that at each
    /// call site produced the same match five times over.
    pub fn author(given: &str, family: &str) -> Self {
        Self {
            creator_type: "author".into(),
            first_name: Some(given.trim().to_string()).filter(|s| !s.is_empty()),
            last_name: Some(family.trim().to_string()).filter(|s| !s.is_empty()),
            name: None,
        }
    }

    /// A name that arrived as one string, and is kept as one.
    ///
    /// Organisations ("World Health Organization"), and people whose names do
    /// not split the way a heuristic would guess. Storing it whole is the only
    /// answer that is never wrong.
    pub fn single(name: &str) -> Self {
        Self {
            creator_type: "author".into(),
            name: Some(name.trim().to_string()).filter(|s| !s.is_empty()),
            ..Default::default()
        }
    }

    pub fn display(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
        match (&self.first_name, &self.last_name) {
            (Some(f), Some(l)) => format!("{f} {l}"),
            (None, Some(l)) => l.clone(),
            (Some(f), None) => f.clone(),
            _ => String::new(),
        }
    }

    /// Family name (or the whole single-field name) — used for sorting and citekeys.
    pub fn sort_name(&self) -> String {
        self.last_name
            .clone()
            .or_else(|| self.name.clone())
            .or_else(|| self.first_name.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ItemTag {
    pub tag: String,
    /// 0 = manual, 1 = automatic (importer / plugin / agent generated).
    #[serde(default)]
    pub r#type: u8,
}

impl ItemTag {
    pub fn manual(tag: impl Into<String>) -> Self {
        Self { tag: tag.into(), r#type: 0 }
    }
    pub fn automatic(tag: impl Into<String>) -> Self {
        Self { tag: tag.into(), r#type: 1 }
    }
}

/// What an item has hanging off it, at the resolution a reader cares about:
/// is there a PDF, is there a saved page, is it only a link out.
///
/// Derived from the child attachments on read and never stored — the
/// attachments themselves are the truth, and a cached summary of them would be
/// one more thing that can disagree with the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Pdf,
    /// A saved copy of a web page.
    Snapshot,
    /// A URL only: nothing was downloaded.
    Link,
    /// Some other file — an image, a dataset, a supplement.
    File,
}

impl AttachmentKind {
    /// Classify one attachment from the two fields Zotero uses to describe it.
    pub fn classify(content_type: Option<&str>, link_mode: Option<&str>) -> Self {
        if link_mode == Some("linked_url") {
            return Self::Link;
        }
        match content_type {
            Some("application/pdf") => Self::Pdf,
            Some("text/html") | Some("application/xhtml+xml") => Self::Snapshot,
            _ => Self::File,
        }
    }

    /// Most telling first, so a row with a PDF and three images reads as "PDF".
    pub const ORDER: [Self; 4] = [Self::Pdf, Self::Snapshot, Self::Link, Self::File];
}

/// A bibliographic item. Notes, attachments and annotations are items too,
/// distinguished by `item_type`, which keeps versioning and tagging uniform.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub key: Key,
    #[serde(rename = "libraryId")]
    pub library_id: i64,
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "parentKey", skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<Key>,
    #[serde(flatten)]
    pub fields: Fields,
    #[serde(default)]
    pub creators: Vec<Creator>,
    #[serde(default)]
    pub tags: Vec<ItemTag>,
    #[serde(default)]
    pub collections: Vec<Key>,
    pub version: i64,
    #[serde(default)]
    pub deleted: bool,
    #[serde(rename = "dateAdded")]
    pub date_added: i64,
    #[serde(rename = "dateModified")]
    pub date_modified: i64,
    /// Derived on read, never written. See [`AttachmentKind`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentKind>,
}

impl Item {
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).and_then(Value::as_str)
    }

    pub fn title(&self) -> &str {
        self.field("title")
            .or_else(|| self.field("caseName"))
            .or_else(|| self.field("subject"))
            .unwrap_or("")
    }

    pub fn date(&self) -> &str {
        self.field("date").unwrap_or("")
    }

    /// First 4-digit run in the date field.
    pub fn year(&self) -> Option<i32> {
        let d = self.date();
        let bytes: Vec<char> = d.chars().collect();
        for w in bytes.windows(4) {
            if w.iter().all(|c| c.is_ascii_digit()) {
                return w.iter().collect::<String>().parse().ok();
            }
        }
        None
    }

    pub fn creator_summary(&self) -> String {
        let names: Vec<String> = self.creators.iter().map(Creator::sort_name).collect();
        match names.len() {
            0 => String::new(),
            1 => names[0].clone(),
            2 => format!("{} & {}", names[0], names[1]),
            _ => format!("{} et al.", names[0]),
        }
    }

    /// Deduplication fingerprint: strongest available identifier, else a
    /// normalised title/author/year triple.
    pub fn fingerprint(&self) -> String {
        for id_field in ["DOI", "ISBN", "PMID", "arXiv"] {
            if let Some(v) = self.field(id_field) {
                let v = text::normalize(v);
                if !v.is_empty() {
                    return format!("{}:{}", id_field.to_lowercase(), v);
                }
            }
        }
        format!(
            "t:{}|a:{}|y:{}",
            text::normalize(self.title()),
            text::normalize(&self.creators.first().map(Creator::sort_name).unwrap_or_default()),
            self.year().map(|y| y.to_string()).unwrap_or_default()
        )
    }

    /// Concatenated text used for indexing.
    pub fn search_text(&self) -> String {
        let mut s = String::with_capacity(256);
        s.push_str(self.title());
        s.push('\n');
        for c in &self.creators {
            s.push_str(&c.display());
            s.push(' ');
        }
        s.push('\n');
        for key in [
            "abstractNote",
            "publicationTitle",
            "bookTitle",
            "proceedingsTitle",
            "publisher",
            "series",
            "extra",
            "note",
            // A highlight is the single most searched-for thing in a library
            // people actually read: "where did I see that phrase?" only works
            // if the passage itself is indexed.
            "annotationText",
            "annotationComment",
        ] {
            if let Some(v) = self.field(key) {
                s.push_str(v);
                s.push('\n');
            }
        }
        for t in &self.tags {
            s.push_str(&t.tag);
            s.push(' ');
        }
        s
    }

    pub fn is_regular(&self) -> bool {
        !matches!(self.item_type.as_str(), "note" | "attachment" | "annotation")
    }
}

/// Payload for creating an item. Server assigns key/version/timestamps.
///
/// Serialisable as well as deserialisable so metadata resolvers can hand a
/// preview back to the client before anything is written.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ItemDraft {
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "parentKey", default, skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<Key>,
    #[serde(flatten, default)]
    pub fields: Fields,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creators: Vec<Creator>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<ItemTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<Key>,
    /// Optional explicit key, used by importers to preserve upstream ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    /// When the item was originally added, for the callers that know.
    ///
    /// An importer does: restoring a backup or migrating from Zotero should
    /// not tell the user they added their whole library today. Everything else
    /// leaves it unset and is stamped with now, which is what "added" means
    /// when a person adds something.
    #[serde(rename = "dateAdded", default, skip_serializing_if = "Option::is_none")]
    pub date_added: Option<i64>,
}

impl ItemDraft {
    pub fn new(item_type: impl Into<String>) -> Self {
        Self { item_type: item_type.into(), ..Default::default() }
    }

    pub fn with_field(mut self, k: &str, v: impl Into<Value>) -> Self {
        self.fields.insert(k.to_string(), v.into());
        self
    }

    pub fn with_creator(mut self, c: Creator) -> Self {
        self.creators.push(c);
        self
    }

    pub fn into_item(self, key: Key, library_id: i64, version: i64) -> Item {
        let ts = now_ms();
        let added = self.date_added.filter(|t| *t > 0).unwrap_or(ts);
        Item {
            key,
            library_id,
            item_type: self.item_type,
            parent_key: self.parent_key,
            fields: self.fields,
            creators: self.creators,
            tags: self.tags,
            collections: self.collections,
            version,
            deleted: false,
            attachments: Vec::new(),
            date_added: added,
            date_modified: ts,
        }
    }
}

/// Sparse update. `None` means "leave untouched"; an explicit `null` inside
/// `fields` clears that field.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemPatch {
    #[serde(rename = "itemType", default)]
    pub item_type: Option<String>,
    #[serde(default)]
    pub fields: Option<Fields>,
    #[serde(default)]
    pub creators: Option<Vec<Creator>>,
    #[serde(default)]
    pub tags: Option<Vec<ItemTag>>,
    #[serde(default)]
    pub collections: Option<Vec<Key>>,
    #[serde(default)]
    pub deleted: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    pub key: Key,
    #[serde(rename = "libraryId")]
    pub library_id: i64,
    pub name: String,
    #[serde(rename = "parentKey", skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<Key>,
    #[serde(rename = "sortIndex")]
    pub sort_index: f64,
    /// Palette name, not a colour value — see `004_collection_appearance.sql`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Name of an icon the app ships; unknown names fall back to a folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub version: i64,
    /// Number of items directly in this collection.
    #[serde(rename = "itemCount", default)]
    pub item_count: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CollectionDraft {
    pub name: String,
    #[serde(rename = "parentKey", default)]
    pub parent_key: Option<Key>,
    #[serde(rename = "sortIndex", default)]
    pub sort_index: Option<f64>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub key: Option<Key>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CollectionPatch {
    pub name: Option<String>,
    /// `Some(None)` clears the colour; absent leaves it alone.
    #[serde(default, deserialize_with = "explicit_null")]
    pub color: Option<Option<String>>,
    #[serde(default, deserialize_with = "explicit_null")]
    pub icon: Option<Option<String>>,
    /// `None` leaves the parent alone; `Some(None)` moves to the top level.
    #[serde(rename = "parentKey", default, deserialize_with = "explicit_null")]
    pub parent_key: Option<Option<Key>>,
    #[serde(rename = "sortIndex")]
    pub sort_index: Option<f64>,
}

/// Distinguishes an absent field from an explicit `null`.
///
/// Serde collapses both to `None` for a plain `Option<Option<T>>`, which would
/// turn "move this to the top level" into "change nothing" — a silent no-op
/// exactly where the user expects a visible move.
fn explicit_null<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// A saved query that behaves like a collection.
///
/// It deliberately stores the *search string* rather than a structured rule
/// tree: users already know the query language from the search box, and reusing
/// it means smart collections cannot drift away from what search actually does.
#[derive(Clone, Debug, Serialize)]
pub struct SmartCollection {
    pub key: Key,
    #[serde(rename = "libraryId")]
    pub library_id: i64,
    pub name: String,
    pub query: String,
    pub mode: String,
    pub sort: String,
    pub direction: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub version: i64,
    /// Filled in on demand; `None` when not evaluated.
    #[serde(rename = "itemCount", skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i64>,
    /// Whether `item_count` is a floor.
    ///
    /// A saved search with no words is a filter and counts exactly. One with
    /// words has to be *run* to be counted, and a ranked search scores a
    /// bounded pool — so "500" in the sidebar might mean five hundred or twenty
    /// thousand, and only this tells them apart.
    #[serde(
        rename = "itemCountApproximate",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub item_count_approximate: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SmartCollectionDraft {
    pub name: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub key: Option<Key>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SmartCollectionPatch {
    pub name: Option<String>,
    pub query: Option<String>,
    #[serde(default, deserialize_with = "explicit_null")]
    pub color: Option<Option<String>>,
    #[serde(default, deserialize_with = "explicit_null")]
    pub icon: Option<Option<String>>,
    pub mode: Option<String>,
    pub sort: Option<String>,
    pub direction: Option<String>,
    #[serde(rename = "sortIndex")]
    pub sort_index: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Tag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub count: i64,
    /// 0 = has at least one manual assignment, 1 = purely automatic.
    pub r#type: u8,
}

/// A chat thread. Messages hang off it; the agent loop lands later, but the
/// history it will read and write is persisted from the start.
#[derive(Clone, Debug, Serialize)]
pub struct Conversation {
    pub key: Key,
    #[serde(rename = "libraryId")]
    pub library_id: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(rename = "messageCount")]
    pub message_count: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    /// `user`, `assistant`, `tool` or `system`.
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    /// Papers this message named with `@`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<Key>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// A slice of a conversation, with a way to ask for the rest.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    /// In reading order, oldest first — the order they are drawn in.
    pub messages: Vec<Message>,
    /// Whether anything older exists. Sent rather than inferred from the page
    /// being full, which is wrong exactly when the thread length is a
    /// multiple of the page size.
    pub has_more: bool,
}

/// What may be changed about a conversation.
///
/// `scope` is doubly optional on purpose: absent means "leave it", and
/// `Some(None)` means "clear it" — the difference between not mentioning the
/// collection and detaching from it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConversationPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub scope: Option<Option<String>>,
}

/// Distinguish an absent field from a null one.
fn double_option<'de, D>(deserializer: D) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MessageDraft {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub meta: Option<Value>,
    /// Papers this message is about, named by the user with `@`.
    ///
    /// Carried separately from the text rather than parsed back out of it:
    /// the client already knows exactly which item was picked, and re-deriving
    /// it from prose would mean guessing at a title the user may have edited.
    #[serde(default)]
    pub mentions: Vec<Key>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Library {
    pub id: i64,
    pub name: String,
    pub r#type: String,
    pub version: i64,
}

/// A page of results plus the total for the same filter.
#[derive(Clone, Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub offset: u32,
    pub limit: u32,
    /// Whether `total` is a floor rather than a count.
    ///
    /// A browse counts every matching row, so it is exact. A ranked search
    /// scores a bounded pool of candidates and stops; its total is "at least
    /// this many", and presenting that as a figure — "100 of 300" for a query
    /// matching twenty thousand — reads as precision that is not there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approximate: bool,
    /// Whether these rows are in relevance order rather than the asked-for one.
    ///
    /// A ranked search cannot honour a column sort: it scores a bounded pool
    /// and returns it best-first, so sorting that pool by title would give the
    /// first title *among the best three hundred*, presented as the first
    /// title in the library. The sort is therefore ignored — and it was
    /// ignored silently, with the table still drawing an arrow on the column
    /// and still accepting clicks that changed nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ranked: bool,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, total: i64, offset: u32, limit: u32) -> Self {
        Self { items, total, offset, limit, approximate: false, ranked: false }
    }

    /// The same page, with its total marked as a lower bound.
    pub fn approximate(mut self) -> Self {
        self.approximate = true;
        self
    }

    /// The same page, marked as ordered by relevance.
    pub fn ranked(mut self) -> Self {
        self.ranked = true;
        self
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            items: self.items.into_iter().map(f).collect(),
            total: self.total,
            offset: self.offset,
            limit: self.limit,
            approximate: self.approximate,
            ranked: self.ranked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, doi: Option<&str>, date: &str) -> Item {
        let mut d = ItemDraft::new("journalArticle").with_field("title", title).with_field("date", date);
        if let Some(doi) = doi {
            d = d.with_field("DOI", doi);
        }
        d.into_item(Key::generate(), 1, 1)
    }

    #[test]
    fn extracts_year() {
        assert_eq!(item("x", None, "2017-06-12").year(), Some(2017));
        assert_eq!(item("x", None, "June 2020").year(), Some(2020));
        assert_eq!(item("x", None, "n.d.").year(), None);
    }

    #[test]
    fn fingerprint_prefers_doi() {
        assert!(item("x", Some("10.1/AB"), "2017").fingerprint().starts_with("doi:"));
        assert!(item("x", None, "2017").fingerprint().starts_with("t:"));
    }

    #[test]
    fn creator_summary_formats() {
        let mut i = item("x", None, "2017");
        i.creators = vec![
            Creator { last_name: Some("Vaswani".into()), ..Default::default() },
            Creator { last_name: Some("Shazeer".into()), ..Default::default() },
            Creator { last_name: Some("Parmar".into()), ..Default::default() },
        ];
        assert_eq!(i.creator_summary(), "Vaswani et al.");
    }
}

#[cfg(test)]
mod patch_tests {
    use super::CollectionPatch;

    #[test]
    fn an_explicit_null_parent_means_move_to_the_top_level() {
        // `Option<Option<_>>` collapses a JSON null to `None` by default, which
        // would silently turn "unparent me" into "leave the parent alone".
        let p: CollectionPatch = serde_json::from_str(r#"{"parentKey":null}"#).unwrap();
        assert_eq!(p.parent_key, Some(None), "null must reach the store as a clear");
    }

    #[test]
    fn an_absent_parent_leaves_the_parent_untouched() {
        let p: CollectionPatch = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert_eq!(p.parent_key, None);
    }
}

#[cfg(test)]
mod search_text_tests {
    use super::*;

    fn with(item_type: &str, fields: &[(&str, &str)]) -> Item {
        let mut draft = ItemDraft::new(item_type);
        for (k, v) in fields {
            draft = draft.with_field(k, *v);
        }
        draft.into_item(Key::generate(), 1, 1)
    }

    #[test]
    fn a_highlight_is_findable_by_its_own_words() {
        let text = with(
            "annotation",
            &[("annotationText", "attention is all you need"), ("annotationComment", "check this")],
        )
        .search_text();

        assert!(text.contains("attention is all you need"));
        assert!(text.contains("check this"));
    }

    #[test]
    fn ordinary_metadata_is_still_indexed() {
        let text = with("journalArticle", &[("title", "Diffusion"), ("abstractNote", "We review")])
            .search_text();
        assert!(text.contains("Diffusion"));
        assert!(text.contains("We review"));
    }
}
