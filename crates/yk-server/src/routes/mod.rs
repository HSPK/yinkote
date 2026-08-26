//! HTTP surface. One module per resource; `router` wires them together.

mod badges;
mod citations;
mod collections;
pub(crate) mod files;
mod import;
mod conversations;
mod items;
mod plugins;
mod scrape;
mod search;
mod smart;
mod summarise;
mod system;

use axum::Router;
use serde::Deserialize;
use serde_json::Value;
use yk_core::event::DomainEvent;
use yk_core::plugin::HookEvent;
use yk_core::query::*;
use yk_core::{Error, Key, Result};

use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new()
        .merge(system::router())
        .merge(items::router())
        .merge(badges::router())
        .merge(citations::router())
        .merge(files::router())
        .merge(import::router())
        .merge(collections::router())
        .merge(conversations::router())
        .merge(search::router())
        .merge(scrape::router())
        .merge(smart::router())
        .merge(summarise::router())
        .merge(plugins::router())
}

/// Look up the library version after a write and broadcast the change.
///
/// Every mutating handler needs exactly this, and getting it wrong (stale
/// version, forgotten event) is invisible until a client fails to resynchronise
/// — so it lives in one place.
async fn announce(
    app: &App,
    lib: i64,
    event: impl FnOnce(i64) -> DomainEvent,
) -> ApiResult<i64> {
    let version = app.store().libraries.version(lib).await?;
    app.events().publish(event(version));
    Ok(version)
}

/// Fire a lifecycle hook without making the client wait for plugins.
fn notify_plugins(app: &App, hook: &'static str, payload: Value) {
    let plugins = app.plugins.clone();
    tokio::spawn(async move {
        plugins.dispatch(HookEvent::new(hook, payload)).await;
    });
}

/// Query string shared by listing and search endpoints.
///
/// Everything is optional and forgiving: an unparseable value falls back to the
/// default rather than 400-ing, because these are user-facing URLs.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    pub q: Option<String>,
    pub mode: Option<String>,
    pub collection: Option<String>,
    pub recursive: Option<bool>,
    /// Comma-separated; a leading `-` excludes.
    pub tag: Option<String>,
    /// Comma-separated item types.
    pub item_type: Option<String>,
    pub top_level: Option<bool>,
    /// `exclude` (default), `only`, `include`.
    pub trash: Option<String>,
    pub since: Option<i64>,
    pub sort: Option<String>,
    pub direction: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub keys: Option<String>,
}

/// A request to order by a plugin-supplied badge column.
pub struct BadgeSort {
    pub plugin_id: String,
    pub badge: String,
}

fn split(value: &Option<String>) -> Vec<String> {
    value
        .iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl ListParams {
    pub fn filter(&self, library_id: i64) -> Result<ItemFilter> {
        let collection = match &self.collection {
            Some(c) if !c.is_empty() => Some(Key::parse(c)?),
            _ => None,
        };
        let keys = match &self.keys {
            Some(_) => {
                Some(split(&self.keys).iter().map(|k| Key::parse(k)).collect::<Result<Vec<_>>>()?)
            }
            None => None,
        };
        Ok(ItemFilter {
            library_id,
            collection,
            recursive: self.recursive.unwrap_or(true),
            tags: split(&self.tag),
            item_types: split(&self.item_type),
            top_level_only: self.top_level.unwrap_or(false),
            trash: match self.trash.as_deref() {
                Some("only") => TrashScope::Only,
                Some("include") => TrashScope::Include,
                _ => TrashScope::Exclude,
            },
            since: self.since,
            keys,
        })
    }

    /// `sort=badge:<pluginId>:<badgeId>`, if that is what was asked for.
    pub fn badge_sort(&self) -> Option<BadgeSort> {
        let rest = self.sort.as_deref()?.strip_prefix("badge:")?;
        let (plugin_id, badge) = rest.split_once(':')?;
        (!plugin_id.is_empty() && !badge.is_empty())
            .then(|| BadgeSort { plugin_id: plugin_id.to_string(), badge: badge.to_string() })
    }

    /// Whether the request asked for descending order, defaulting to it: a
    /// badge sort is nearly always "show me the best first".
    pub fn descending(&self) -> bool {
        self.direction.as_deref() != Some("asc")
    }

    pub fn query(&self, library_id: i64) -> Result<ItemQuery> {
        Ok(ItemQuery {
            filter: self.filter(library_id)?,
            sort: self.sort.as_deref().and_then(SortField::parse).unwrap_or_default(),
            direction: match self.direction.as_deref() {
                Some("asc") => Direction::Asc,
                Some("desc") => Direction::Desc,
                // Alphabetical fields read better ascending.
                _ => match self.sort.as_deref() {
                    Some("title") | Some("creator") | Some("itemType") => Direction::Asc,
                    _ => Direction::Desc,
                },
            },
            limit: self.limit.unwrap_or(DEFAULT_LIMIT),
            offset: self.offset.unwrap_or(0),
        }
        .clamped())
    }

    pub fn search_mode(&self) -> SearchMode {
        self.mode.as_deref().and_then(SearchMode::parse).unwrap_or_default()
    }

    pub fn text(&self) -> &str {
        self.q.as_deref().unwrap_or("").trim()
    }
}

/// Parse a path key, turning a malformed one into a 422 rather than a 500.
pub fn key(raw: &str) -> Result<Key> {
    Key::parse(raw).map_err(|_| Error::invalid(format!("malformed key '{raw}'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_comma_lists_and_ignores_blanks() {
        assert_eq!(split(&Some("a, b ,,c".into())), vec!["a", "b", "c"]);
        assert!(split(&None).is_empty());
    }

    #[test]
    fn filter_defaults_are_sensible() {
        let f = ListParams::default().filter(1).unwrap();
        assert_eq!(f.library_id, 1);
        assert!(f.recursive, "browsing a collection should include sub-collections");
        assert_eq!(f.trash, TrashScope::Exclude);
    }

    #[test]
    fn trash_scope_is_parsed() {
        let p = ListParams { trash: Some("only".into()), ..Default::default() };
        assert_eq!(p.filter(1).unwrap().trash, TrashScope::Only);
    }

    #[test]
    fn title_sort_defaults_to_ascending() {
        let p = ListParams { sort: Some("title".into()), ..Default::default() };
        let q = p.query(1).unwrap();
        assert_eq!(q.sort, SortField::Title);
        assert_eq!(q.direction, Direction::Asc);
    }

    #[test]
    fn recency_sort_defaults_to_descending() {
        assert_eq!(ListParams::default().query(1).unwrap().direction, Direction::Desc);
    }

    #[test]
    fn limit_is_clamped() {
        let p = ListParams { limit: Some(99_999), ..Default::default() };
        assert_eq!(p.query(1).unwrap().limit, MAX_LIMIT);
    }

    #[test]
    fn malformed_keys_are_rejected() {
        let p = ListParams { collection: Some("bad key!".into()), ..Default::default() };
        assert!(p.filter(1).is_err());
    }
}
