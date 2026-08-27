//! Per-item badges contributed by plugins.
//!
//! The host knows nothing about impact factors or journal tiers, and should
//! not: those datasets are licensed, regional and change yearly. What the host
//! owns is the shape of the exchange — which items need annotating, which
//! fields a plugin is allowed to see, and how long an answer stays good.
//!
//! Answers are cached against the item's version, so an edit invalidates
//! exactly the affected rows and nothing else. A resolver that fails is skipped
//! rather than failing the request: a missing badge is a blank cell, not a
//! broken table.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;
use yk_core::model::Item;
use yk_core::plugin::{BadgeDescriptor, BadgeValue};
use yk_core::ports::PluginHost;

/// Bounded cache of resolved badges.
///
/// Keyed by item version as well as key, so staleness is impossible rather than
/// merely unlikely; the cap keeps a large library from pinning memory.
const CACHE_CAP: usize = 20_000;

type CacheKey = (String, String, i64);

pub struct BadgeService {
    plugins: Arc<dyn PluginHost>,
    cache: Mutex<HashMap<CacheKey, Vec<BadgeValue>>>,
}

impl BadgeService {
    pub fn new(plugins: Arc<dyn PluginHost>) -> Self {
        Self { plugins, cache: Mutex::new(HashMap::new()) }
    }

    /// Every badge column currently on offer.
    pub async fn descriptors(&self) -> Vec<BadgeDescriptor> {
        self.plugins.contributions().await.badges
    }

    /// Resolve badges for a batch of items, grouped by item key.
    pub async fn resolve(&self, items: &[Item]) -> HashMap<String, Vec<BadgeValue>> {
        let descriptors = self.descriptors().await;
        if descriptors.is_empty() || items.is_empty() {
            return HashMap::new();
        }

        let mut out: HashMap<String, Vec<BadgeValue>> = HashMap::new();
        for (plugin_id, wanted) in group_by_plugin(&descriptors) {
            let (cached, missing) = self.partition(&plugin_id, items).await;
            for (key, values) in cached {
                out.entry(key).or_default().extend(values);
            }
            if missing.is_empty() {
                continue;
            }

            let fresh = self.ask(&plugin_id, &wanted, &missing).await;
            self.remember(&plugin_id, &missing, &fresh).await;
            for (key, values) in fresh {
                out.entry(key).or_default().extend(values);
            }
        }
        out
    }

    /// Split a batch into what is already known and what must be asked for.
    async fn partition(
        &self,
        plugin_id: &str,
        items: &[Item],
    ) -> (HashMap<String, Vec<BadgeValue>>, Vec<Item>) {
        let cache = self.cache.lock().await;
        let mut hits = HashMap::new();
        let mut misses = Vec::new();
        for item in items {
            let key = (plugin_id.to_string(), item.key.to_string(), item.version);
            match cache.get(&key) {
                Some(values) if !values.is_empty() => {
                    hits.insert(item.key.to_string(), values.clone());
                }
                // A known-empty answer still counts as known: re-asking a
                // plugin about an item it has nothing to say about is the most
                // common request there is.
                Some(_) => {}
                None => misses.push(item.clone()),
            }
        }
        (hits, misses)
    }

    async fn ask(
        &self,
        plugin_id: &str,
        wanted: &[&BadgeDescriptor],
        items: &[Item],
    ) -> HashMap<String, Vec<BadgeValue>> {
        let needs: Vec<&str> =
            wanted.iter().flat_map(|d| d.needs.iter()).map(String::as_str).collect();
        let payload = json!({
            "badges": wanted.iter().map(|d| &d.id).collect::<Vec<_>>(),
            "items": items.iter().map(|i| summarise(i, &needs)).collect::<Vec<_>>(),
        });

        match self.plugins.call(plugin_id, "badges.resolve", payload).await {
            Ok(value) => parse_answer(plugin_id, value),
            Err(error) => {
                tracing::warn!(plugin = plugin_id, %error, "badge resolution failed");
                HashMap::new()
            }
        }
    }

    async fn remember(
        &self,
        plugin_id: &str,
        asked: &[Item],
        answers: &HashMap<String, Vec<BadgeValue>>,
    ) {
        let mut cache = self.cache.lock().await;
        if cache.len() > CACHE_CAP {
            cache.clear();
        }
        for item in asked {
            let values = answers.get(item.key.as_str()).cloned().unwrap_or_default();
            cache.insert((plugin_id.to_string(), item.key.to_string(), item.version), values);
        }
    }

    /// Drop everything, for when plugins are reloaded and may answer differently.
    pub async fn clear(&self) {
        self.cache.lock().await.clear();
    }
}

/// Only the fields a plugin declared it needs, plus the key it must echo back.
///
/// A badge plugin has no reason to see abstracts or notes, and not sending them
/// is cheaper than trusting it not to look.
fn summarise(item: &Item, needs: &[&str]) -> Value {
    let mut fields = serde_json::Map::new();
    for name in needs {
        if let Some(value) = item.fields.get(*name) {
            fields.insert((*name).to_string(), value.clone());
        }
    }
    json!({ "key": item.key.as_str(), "itemType": item.item_type, "fields": fields })
}

fn group_by_plugin(descriptors: &[BadgeDescriptor]) -> Vec<(String, Vec<&BadgeDescriptor>)> {
    let mut grouped: Vec<(String, Vec<&BadgeDescriptor>)> = Vec::new();
    for d in descriptors {
        match grouped.iter_mut().find(|(id, _)| *id == d.plugin_id) {
            Some((_, list)) => list.push(d),
            None => grouped.push((d.plugin_id.clone(), vec![d])),
        }
    }
    grouped
}

/// Accepts `{ "ITEMKEY": [ {badge, text, tone?, title?} ] }`.
fn parse_answer(plugin_id: &str, value: Value) -> HashMap<String, Vec<BadgeValue>> {
    let source = value.get("badges").cloned().unwrap_or(value);
    let Some(map) = source.as_object() else { return HashMap::new() };

    map.iter()
        .filter_map(|(key, list)| {
            let values: Vec<BadgeValue> = serde_json::from_value::<Vec<BadgeValue>>(list.clone())
                .ok()?
                .into_iter()
                .map(|mut v| {
                    v.plugin_id = plugin_id.to_string();
                    v
                })
                .collect();
            (!values.is_empty()).then(|| (key.clone(), values))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use yk_core::plugin::{Contributions, HookEvent, HookOutcome, PluginStatus};
    use yk_core::Result;

    #[derive(Default)]
    struct FakeHost {
        badges: Vec<BadgeDescriptor>,
        calls: Arc<Mutex<Vec<Value>>>,
        fail: bool,
    }

    #[async_trait]
    impl PluginHost for FakeHost {
        async fn list(&self) -> Vec<PluginStatus> {
            Vec::new()
        }
        async fn get(&self, _: &str) -> Result<PluginStatus> {
            Err(yk_core::Error::not_found("no"))
        }
        async fn set_enabled(&self, _: &str, _: bool) -> Result<PluginStatus> {
            Err(yk_core::Error::not_found("no"))
        }
        async fn reload(&self) -> Result<()> {
            Ok(())
        }
        async fn contributions(&self) -> Contributions {
            Contributions { badges: self.badges.clone(), ..Default::default() }
        }
        async fn call(&self, _: &str, _: &str, params: Value) -> Result<Value> {
            self.calls.lock().await.push(params.clone());
            if self.fail {
                return Err(yk_core::Error::internal("plugin exploded"));
            }
            let items = params["items"].as_array().cloned().unwrap_or_default();
            let mut out = serde_json::Map::new();
            for item in items {
                let key = item["key"].as_str().unwrap_or_default().to_string();
                if let Some(issn) = item["fields"].get("ISSN").and_then(Value::as_str) {
                    // The ISSN's leading digit stands in for a metric, so a
                    // test can control the ranking it expects.
                    let rank: f64 = issn.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(0)
                        as f64;
                    out.insert(
                        key,
                        json!([{ "badge": "if", "text": "12.3", "rank": rank, "tone": "violet" }]),
                    );
                }
            }
            Ok(json!({ "badges": out }))
        }
        async fn dispatch(&self, _: HookEvent) -> Vec<HookOutcome> {
            Vec::new()
        }
        async fn shutdown(&self) {}
    }

    fn host(fail: bool) -> (Arc<FakeHost>, Arc<Mutex<Vec<Value>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let host = Arc::new(FakeHost {
            badges: vec![BadgeDescriptor {
                id: "if".into(),
                label: "IF".into(),
                description: None,
                needs: vec!["ISSN".into()],
                width: None,
                sortable: true,
                plugin_id: "metrics".into(),
            }],
            calls: calls.clone(),
            fail,
        });
        (host, calls)
    }

    fn item(key: &str, issn: Option<&str>) -> Item {
        let mut fields = yk_core::model::Fields::new();
        if let Some(issn) = issn {
            fields.insert("ISSN".into(), json!(issn));
        }
        fields.insert("abstractNote".into(), json!("secret"));
        Item {
            key: key.parse().unwrap(),
            library_id: 1,
            item_type: "journalArticle".into(),
            parent_key: None,
            fields,
            creators: Vec::new(),
            tags: Vec::new(),
            collections: Vec::new(),
            version: 1,
            deleted: false,
            attachments: Vec::new(),
            date_added: 0,
            date_modified: 0,
        }
    }

    #[tokio::test]
    async fn resolves_and_stamps_ownership() {
        let (host, _) = host(false);
        let svc = BadgeService::new(host);
        let got = svc.resolve(&[item("AAA", Some("1234-5678"))]).await;
        assert_eq!(got["AAA"][0].text, "12.3");
        assert_eq!(got["AAA"][0].plugin_id, "metrics", "the UI must know who answered");
    }

    #[tokio::test]
    async fn sends_only_the_fields_a_plugin_declared() {
        let (host, calls) = host(false);
        let svc = BadgeService::new(host);
        svc.resolve(&[item("AAA", Some("1234-5678"))]).await;
        let sent = &calls.lock().await[0]["items"][0]["fields"];
        assert!(sent.get("ISSN").is_some());
        assert!(sent.get("abstractNote").is_none(), "undeclared fields must not leave the host");
    }

    #[tokio::test]
    async fn a_second_pass_asks_nothing() {
        let (host, calls) = host(false);
        let svc = BadgeService::new(host);
        let items = [item("AAA", Some("1234-5678")), item("BBB", None)];
        svc.resolve(&items).await;
        let got = svc.resolve(&items).await;
        assert_eq!(got["AAA"][0].text, "12.3");
        assert_eq!(calls.lock().await.len(), 1, "cached items, including empty answers");
    }

    #[tokio::test]
    async fn editing_an_item_invalidates_only_that_row() {
        let (host, calls) = host(false);
        let svc = BadgeService::new(host);
        let mut items = vec![item("AAA", Some("1234-5678")), item("BBB", Some("9999-0000"))];
        svc.resolve(&items).await;

        items[0].version += 1;
        svc.resolve(&items).await;

        let asked = calls.lock().await;
        assert_eq!(asked.len(), 2);
        let second = asked[1]["items"].as_array().unwrap();
        assert_eq!(second.len(), 1, "only the edited item is re-asked");
        assert_eq!(second[0]["key"], "AAA");
    }

    #[tokio::test]
    async fn carries_the_rank_and_colour_the_plugin_chose() {
        // The host cannot know that Q1 beats Q4 or that tier 1 is the best, so
        // the plugin ranks and colours its own values and the host only orders.
        let (host, _) = host(false);
        let svc = BadgeService::new(host);
        let got = svc.resolve(&[item("AAA", Some("9000-0000"))]).await;
        assert_eq!(got["AAA"][0].rank, Some(9.0));
        assert_eq!(got["AAA"][0].tone.as_deref(), Some("violet"));
    }

    #[tokio::test]
    async fn a_failing_plugin_leaves_a_blank_cell_not_an_error() {
        let (host, _) = host(true);
        let svc = BadgeService::new(host);
        assert!(svc.resolve(&[item("AAA", Some("1234-5678"))]).await.is_empty());
    }

    #[tokio::test]
    async fn no_badge_plugins_means_no_calls() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let host = Arc::new(FakeHost { badges: Vec::new(), calls: calls.clone(), fail: false });
        let svc = BadgeService::new(host);
        assert!(svc.resolve(&[item("AAA", Some("1"))]).await.is_empty());
        assert!(calls.lock().await.is_empty());
    }
}
