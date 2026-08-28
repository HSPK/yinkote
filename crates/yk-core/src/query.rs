use serde::{Deserialize, Serialize};

use crate::Key;

pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortField {
    #[default]
    DateModified,
    DateAdded,
    Title,
    Creator,
    Year,
    ItemType,
    /// What the row has attached: a PDF sorts above a saved page, above a
    /// bare link, above nothing. One ordering answers both "which of these
    /// have files" and "which have the good kind".
    Attachment,
    /// Only meaningful for search results.
    Relevance,
}

impl SortField {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "dateModified" | "modified" => Self::DateModified,
            "dateAdded" | "added" => Self::DateAdded,
            "title" => Self::Title,
            "creator" => Self::Creator,
            "year" | "date" => Self::Year,
            "itemType" | "type" => Self::ItemType,
            "attachment" | "attachments" => Self::Attachment,
            "relevance" | "score" => Self::Relevance,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Asc,
    #[default]
    Desc,
}

impl Direction {
    pub fn sql(self) -> &'static str {
        match self {
            Direction::Asc => "ASC",
            Direction::Desc => "DESC",
        }
    }
}

/// Which slice of the library a listing or search should consider.
#[derive(Clone, Debug, Default)]
pub struct ItemFilter {
    pub library_id: i64,
    pub collection: Option<Key>,
    /// Include items in descendant collections.
    pub recursive: bool,
    /// AND-ed tag names. A leading `-` negates.
    pub tags: Vec<String>,
    pub item_types: Vec<String>,
    pub top_level_only: bool,
    pub trash: TrashScope,
    /// Only objects with `version > since`.
    pub since: Option<i64>,
    pub keys: Option<Vec<Key>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrashScope {
    /// Exclude trashed items (normal browsing).
    #[default]
    Exclude,
    /// Only trashed items.
    Only,
    /// Both — used by sync.
    Include,
}

#[derive(Clone, Debug)]
pub struct ItemQuery {
    pub filter: ItemFilter,
    pub sort: SortField,
    pub direction: Direction,
    pub limit: u32,
    pub offset: u32,
}

impl Default for ItemQuery {
    fn default() -> Self {
        Self {
            filter: ItemFilter::default(),
            sort: SortField::default(),
            direction: Direction::default(),
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

impl ItemQuery {
    pub fn clamped(mut self) -> Self {
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        self
    }
}

/// How the search text should be interpreted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Lexical BM25 over the full-text index.
    Keyword,
    /// Typo-tolerant matching over titles/creators.
    Fuzzy,
    /// Embedding cosine similarity.
    Semantic,
    /// Reciprocal-rank fusion of all of the above.
    #[default]
    Hybrid,
}

impl SearchMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "keyword" | "lexical" | "bm25" => Self::Keyword,
            "fuzzy" => Self::Fuzzy,
            "semantic" | "vector" => Self::Semantic,
            "hybrid" | "auto" => Self::Hybrid,
            _ => return None,
        })
    }

    pub fn uses(self, other: SearchMode) -> bool {
        self == SearchMode::Hybrid || self == other
    }
}

#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub text: String,
    pub mode: SearchMode,
    pub filter: ItemFilter,
    pub limit: u32,
    pub offset: u32,
    pub highlight: bool,
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            text: String::new(),
            mode: SearchMode::default(),
            filter: ItemFilter::default(),
            limit: DEFAULT_LIMIT,
            offset: 0,
            highlight: true,
        }
    }
}

/// Which retrieval strategy produced a hit; useful for explainability in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchSource {
    Keyword,
    Fuzzy,
    Semantic,
    Tag,
    Field,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub key: Key,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub sources: Vec<MatchSource>,
}

/// A page of results, and how many there were to page through.
///
/// The count is separate from the page because it cannot be derived from it,
/// and deriving it anyway is exactly the bug this type exists to prevent: a
/// listing that reported `hits.len()` as its total told the client there was
/// nothing after the first page, so scrolling a search stopped at one screen.
///
/// It is a count of *candidates*, not of matches in the library. A ranked
/// search scores a bounded pool and stops; saying "at least this many" is
/// honest, and it is what paging needs.
#[derive(Debug, Clone, Default)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub total: i64,
    /// Whether a retriever filled its candidate pool, which makes `total` a
    /// floor rather than a count. A query matching twenty thousand documents
    /// and one matching exactly three hundred both report three hundred; only
    /// this tells them apart.
    pub capped: bool,
}

/// A document as seen by the search index — flat, denormalised, cheap to score.
#[derive(Clone, Debug)]
pub struct SearchDoc {
    pub key: Key,
    pub library_id: i64,
    pub item_type: String,
    pub title: String,
    pub creators: String,
    pub year: Option<i32>,
    pub tags: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SearchStats {
    pub documents: i64,
    pub embedded: i64,
    pub dimensions: usize,
    pub provider: String,
}
