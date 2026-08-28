//! The host API exposed *to* plugins.
//!
//! Every method is permission-gated. Plugins are ordinary API consumers with a
//! narrower surface — they get no privileged access whatsoever.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use yk_core::event::DomainEvent;
use yk_core::model::ItemDraft;
use yk_core::plugin::Permission;
use yk_core::ports::HostApi;
use yk_core::query::{ItemFilter, SearchMode, SearchRequest};
use yk_core::{Error, Key, Result};

use crate::state::Services;

pub struct HostBridge {
    services: Arc<Services>,
}

impl HostBridge {
    pub fn new(services: Arc<Services>) -> Arc<Self> {
        Arc::new(Self { services })
    }

    fn require(granted: &[Permission], needed: Permission) -> Result<()> {
        if granted.contains(&needed) {
            return Ok(());
        }
        Err(Error::Forbidden(format!("plugin lacks permission to {}", needed.label())))
    }

    fn library(&self, params: &Value) -> i64 {
        params
            .get("libraryId")
            .and_then(Value::as_i64)
            .unwrap_or(self.services.default_library)
    }

    /// Plugin settings are namespaced so one plugin cannot read another's keys.
    fn settings_key(plugin_id: &str, key: &str) -> String {
        format!("plugin.{plugin_id}.{key}")
    }
}

#[async_trait]
impl HostApi for HostBridge {
    async fn invoke(
        &self,
        plugin_id: &str,
        granted: &[Permission],
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let s = &self.services;
        match method {
            "host.version" => Ok(json!({ "version": env!("CARGO_PKG_VERSION") })),

            "host.log" => {
                let level = params.get("level").and_then(Value::as_str).unwrap_or("info");
                let message =
                    params.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
                tracing::info!(plugin = plugin_id, level, "{message}");
                Ok(Value::Null)
            }

            "host.notify" => {
                Self::require(granted, Permission::Notify)?;
                let message =
                    params.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
                s.events.publish(DomainEvent::Log { level: "info".into(), message });
                Ok(Value::Null)
            }

            "host.items.search" => {
                Self::require(granted, Permission::Search)?;
                let request = SearchRequest {
                    text: params.get("q").and_then(Value::as_str).unwrap_or_default().to_string(),
                    mode: params
                        .get("mode")
                        .and_then(Value::as_str)
                        .and_then(SearchMode::parse)
                        .unwrap_or_default(),
                    filter: ItemFilter { library_id: self.library(&params), ..Default::default() },
                    limit: params.get("limit").and_then(Value::as_u64).unwrap_or(20) as u32,
                    offset: 0,
                    highlight: false,
                };
                // The hits only. A plugin was promised an array here, and
                // widening it to an object would break every one already
                // written against it.
                Ok(json!(s.search.search(&request).await?.hits))
            }

            "host.items.get" => {
                Self::require(granted, Permission::ItemsRead)?;
                let key = params
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::invalid("key is required"))?;
                let item = s.store.items.get(self.library(&params), &Key::parse(key)?).await?;
                Ok(serde_json::to_value(item)?)
            }

            "host.items.list" => {
                Self::require(granted, Permission::ItemsRead)?;
                let query = yk_core::query::ItemQuery {
                    filter: ItemFilter { library_id: self.library(&params), ..Default::default() },
                    limit: params.get("limit").and_then(Value::as_u64).unwrap_or(50) as u32,
                    ..Default::default()
                }
                .clamped();
                Ok(serde_json::to_value(s.store.items.list(&query).await?)?)
            }

            "host.items.create" => {
                Self::require(granted, Permission::ItemsWrite)?;
                let raw = params
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Error::invalid("items must be an array"))?;
                let drafts: Vec<ItemDraft> = raw
                    .iter()
                    .map(|v| serde_json::from_value(v.clone()))
                    .collect::<std::result::Result<_, _>>()?;
                let library_id = self.library(&params);
                let results = s.store.items.create_many(library_id, drafts).await?;

                let created: Vec<Key> =
                    results.iter().filter_map(|r| r.as_ref().ok().map(|i| i.key.clone())).collect();
                if !created.is_empty() {
                    let version = s.store.libraries.version(library_id).await?;
                    s.events.publish(DomainEvent::ItemsChanged {
                        library_id,
                        keys: created.clone(),
                        version,
                    });
                }
                Ok(json!({
                    "created": created,
                    "failed": results.iter().filter(|r| r.is_err()).count(),
                }))
            }

            // Plugins get the same identifier engine the UI uses, so a plugin
            // never has to reimplement DOI/arXiv parsing.
            "host.resolve" => {
                Self::require(granted, Permission::Network)?;
                let text = params.get("text").and_then(Value::as_str).unwrap_or_default();
                let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(3) as usize;
                Ok(json!(s.scrape.resolve_text(text, limit.min(10)).await))
            }

            "host.collections.list" => {
                Self::require(granted, Permission::CollectionsRead)?;
                Ok(serde_json::to_value(s.store.collections.list(self.library(&params)).await?)?)
            }

            "host.tags.list" => {
                Self::require(granted, Permission::ItemsRead)?;
                Ok(serde_json::to_value(
                    s.store.tags.list(self.library(&params), None, 200).await?,
                )?)
            }

            "host.settings.get" => {
                Self::require(granted, Permission::Settings)?;
                let key = params.get("key").and_then(Value::as_str).unwrap_or_default();
                Ok(s.store
                    .settings
                    .get(&Self::settings_key(plugin_id, key))
                    .await?
                    .unwrap_or(Value::Null))
            }

            "host.settings.set" => {
                Self::require(granted, Permission::Settings)?;
                let key = params.get("key").and_then(Value::as_str).unwrap_or_default();
                let value = params.get("value").cloned().unwrap_or(Value::Null);
                s.store.settings.set(&Self::settings_key(plugin_id, key), &value).await?;
                Ok(Value::Null)
            }

            other => Err(Error::not_found(format!("host method '{other}'"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yk_core::ports::SearchIndex;
    use yk_search::{LocalEmbedder, SearchEngine};
    use yk_store::Store;

    fn services() -> Arc<Services> {
        let store = Store::in_memory().unwrap();
        let search: Arc<dyn SearchIndex> =
            Arc::new(SearchEngine::new(store.clone(), Arc::new(LocalEmbedder::new())).unwrap());
        Arc::new(Services {
            default_library: store.default_library,
            store,
            search,
            scrape: Arc::new(yk_scrape::ScrapeEngine::with_defaults()),
            storage: Arc::new(crate::storage::Storage::new(std::env::temp_dir().join("yk-test"))),
            events: yk_core::event::EventBus::default(),
        })
    }

    #[tokio::test]
    async fn denies_calls_without_permission() {
        let bridge = HostBridge::new(services());
        let err = bridge.invoke("p", &[], "host.items.get", json!({"key":"ABCD1234"})).await.unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Forbidden);
    }

    #[tokio::test]
    async fn unknown_methods_are_not_found() {
        let bridge = HostBridge::new(services());
        let err = bridge.invoke("p", &[], "host.rm.rf", json!({})).await.unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn logging_needs_no_permission() {
        let bridge = HostBridge::new(services());
        assert!(bridge
            .invoke("p", &[], "host.log", json!({"message":"hi"}))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn create_then_read_back_with_permissions() {
        let svc = services();
        let bridge = HostBridge::new(svc.clone());
        let out = bridge
            .invoke(
                "p",
                &[Permission::ItemsWrite],
                "host.items.create",
                json!({"items":[{"itemType":"journalArticle","title":"From a plugin"}]}),
            )
            .await
            .unwrap();
        assert_eq!(out["created"].as_array().unwrap().len(), 1);

        let key = out["created"][0].as_str().unwrap().to_string();
        let item = bridge
            .invoke("p", &[Permission::ItemsRead], "host.items.get", json!({"key": key}))
            .await
            .unwrap();
        assert_eq!(item["title"], "From a plugin");
    }

    #[tokio::test]
    async fn settings_are_namespaced_per_plugin() {
        let svc = services();
        let bridge = HostBridge::new(svc.clone());
        bridge
            .invoke("alpha", &[Permission::Settings], "host.settings.set", json!({"key":"k","value":1}))
            .await
            .unwrap();
        let mine = bridge
            .invoke("alpha", &[Permission::Settings], "host.settings.get", json!({"key":"k"}))
            .await
            .unwrap();
        let theirs = bridge
            .invoke("beta", &[Permission::Settings], "host.settings.get", json!({"key":"k"}))
            .await
            .unwrap();
        assert_eq!(mine, json!(1));
        assert_eq!(theirs, Value::Null);
    }
}
