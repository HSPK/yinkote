//! The plugin registry: discovery, lifecycle, capability aggregation and the
//! hook bus.
//!
//! The registry is the only thing the rest of the application sees (via
//! [`yk_core::ports::PluginHost`]), so runtimes — child process today, WASM
//! tomorrow — can be added without touching callers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use yk_core::plugin::*;
use yk_core::ports::{HostApi, PluginHost};
use yk_core::{Error, Result};

use crate::manifest::{self, Discovered};
use crate::process::PluginProcess;

/// A plugin compiled into the host binary. Same protocol surface, no process.
#[async_trait]
pub trait BuiltinPlugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    async fn call(&self, method: &str, params: Value) -> Result<Value>;
}

enum Instance {
    Process(Arc<PluginProcess>),
    Builtin(Arc<dyn BuiltinPlugin>),
}

impl Instance {
    async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        match self {
            Instance::Process(p) => p.call(method, params, timeout).await,
            Instance::Builtin(b) => match tokio::time::timeout(timeout, b.call(method, params)).await
            {
                Ok(r) => r,
                Err(_) => Err(Error::plugin("builtin plugin timed out")),
            },
        }
    }

    async fn stop(&self) {
        if let Instance::Process(p) = self {
            p.stop().await;
        }
    }

    fn is_alive(&self) -> bool {
        match self {
            Instance::Process(p) => p.is_alive(),
            Instance::Builtin(_) => true,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Stats {
    calls: u64,
    failures: u64,
    total_ms: u64,
}

struct Entry {
    manifest: PluginManifest,
    dir: PathBuf,
    source: String,
    state: PluginState,
    contributions: Contributions,
    instance: Option<Arc<Instance>>,
    stats: Stats,
}

impl Entry {
    fn status(&self) -> PluginStatus {
        PluginStatus {
            manifest: self.manifest.clone(),
            // Everything except an explicit disable counts as on: a plugin that
            // is merely stopped or still starting has not been turned off, and
            // one that failed was trying.
            enabled: !matches!(self.state, PluginState::Disabled),
            state: self.state.clone(),
            contributions: self.contributions.clone(),
            calls: self.stats.calls,
            failures: self.stats.failures,
            avg_latency_ms: if self.stats.calls == 0 {
                0.0
            } else {
                self.stats.total_ms as f64 / self.stats.calls as f64
            },
            source: self.source.clone(),
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.manifest.timeout_ms)
    }
}

pub struct PluginRegistry {
    dirs: Vec<PathBuf>,
    builtins: Vec<Arc<dyn BuiltinPlugin>>,
    host_api: Arc<dyn HostApi>,
    entries: RwLock<HashMap<String, Entry>>,
    /// Ids explicitly turned off by the user; survives reloads.
    disabled: RwLock<std::collections::HashSet<String>>,
    /// Non-fatal problems from the last discovery pass, surfaced in the UI.
    diagnostics: RwLock<Vec<String>>,
}

impl PluginRegistry {
    /// Discover, then start everything that is enabled.
    pub async fn load(&self) -> Result<()> {
        let (found, errors) = manifest::discover(&self.dirs);
        *self.diagnostics.write().await = errors;

        let mut discovered: Vec<Discovered> = found;
        for b in &self.builtins {
            let m = b.manifest();
            discovered.push(Discovered {
                dir: PathBuf::from("."),
                source: "builtin".into(),
                manifest: m,
            });
        }

        let disabled = self.disabled.read().await.clone();
        let mut entries = HashMap::new();
        for d in discovered {
            let enabled = d.manifest.enabled && !disabled.contains(&d.manifest.id);
            entries.insert(
                d.manifest.id.clone(),
                Entry {
                    state: if enabled { PluginState::Stopped } else { PluginState::Disabled },
                    contributions: Contributions::default(),
                    instance: None,
                    stats: Stats::default(),
                    manifest: d.manifest,
                    dir: d.dir,
                    source: d.source,
                },
            );
        }
        *self.entries.write().await = entries;

        let ids: Vec<String> = self.entries.read().await.keys().cloned().collect();
        for id in ids {
            let should_start = matches!(
                self.entries.read().await.get(&id).map(|e| e.state.clone()),
                Some(PluginState::Stopped)
            );
            if should_start {
                if let Err(e) = self.start(&id).await {
                    tracing::warn!(plugin = %id, error = %e, "plugin failed to start");
                }
            }
        }
        Ok(())
    }

    pub async fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.read().await.clone()
    }

    /// Start one plugin and perform the `initialize` handshake.
    async fn start(&self, id: &str) -> Result<()> {
        let (manifest, dir) = {
            let entries = self.entries.read().await;
            let e = entries.get(id).ok_or_else(|| Error::not_found(format!("plugin {id}")))?;
            (e.manifest.clone(), e.dir.clone())
        };

        self.set_state(id, PluginState::Starting).await;

        let instance = match &manifest.runtime {
            PluginRuntime::Process { .. } => {
                match PluginProcess::start(&manifest, &dir, self.host_api.clone()).await {
                    Ok(p) => Instance::Process(p),
                    Err(e) => {
                        self.set_state(id, PluginState::Failed { error: e.to_string() }).await;
                        return Err(e);
                    }
                }
            }
            PluginRuntime::Builtin => {
                let b = self
                    .builtins
                    .iter()
                    .find(|b| b.manifest().id == id)
                    .ok_or_else(|| Error::plugin(format!("builtin '{id}' not registered")))?;
                Instance::Builtin(b.clone())
            }
        };

        let handshake = json!({
            "apiVersion": PLUGIN_API_VERSION,
            "hostVersion": env!("CARGO_PKG_VERSION"),
            "pluginId": manifest.id,
            "permissions": manifest.permissions,
        });

        let started = Instant::now();
        let handshake_result = instance
            .call("initialize", handshake, Duration::from_millis(manifest.timeout_ms))
            .await;
        self.record(id, started.elapsed(), handshake_result.is_ok()).await;

        let contributions = match handshake_result {
            Ok(v) => parse_contributions(&manifest.id, v),
            Err(e) => {
                instance.stop().await;
                self.set_state(id, PluginState::Failed { error: e.to_string() }).await;
                return Err(e);
            }
        };

        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(id) {
            entry.instance = Some(Arc::new(instance));
            entry.contributions = contributions;
            entry.state = PluginState::Ready;
        }
        tracing::info!(plugin = %id, "plugin ready");
        Ok(())
    }

    async fn stop(&self, id: &str) {
        let instance = {
            let mut entries = self.entries.write().await;
            match entries.get_mut(id) {
                Some(e) => {
                    e.contributions = Contributions::default();
                    e.instance.take()
                }
                None => None,
            }
        };
        if let Some(i) = instance {
            i.stop().await;
        }
    }

    async fn set_state(&self, id: &str, state: PluginState) {
        if let Some(e) = self.entries.write().await.get_mut(id) {
            e.state = state;
        }
    }

    /// Resolve a live instance, restarting a crashed one on demand.
    async fn instance_of(&self, id: &str) -> Result<(Arc<Instance>, Duration)> {
        {
            let entries = self.entries.read().await;
            let e = entries.get(id).ok_or_else(|| Error::not_found(format!("plugin {id}")))?;
            if let Some(i) = &e.instance {
                if i.is_alive() {
                    return Ok((i.clone(), e.timeout()));
                }
            }
            if matches!(e.state, PluginState::Disabled) {
                return Err(Error::Forbidden(format!("plugin '{id}' is disabled")));
            }
        }
        // Crashed or never started: one restart attempt, then give up.
        self.stop(id).await;
        self.start(id).await?;
        let entries = self.entries.read().await;
        let e = entries.get(id).ok_or_else(|| Error::not_found(format!("plugin {id}")))?;
        let i = e.instance.clone().ok_or_else(|| Error::plugin(format!("plugin '{id}' is down")))?;
        Ok((i, e.timeout()))
    }

    async fn record(&self, id: &str, elapsed: Duration, ok: bool) {
        if let Some(e) = self.entries.write().await.get_mut(id) {
            e.stats.calls += 1;
            e.stats.total_ms += elapsed.as_millis() as u64;
            if !ok {
                e.stats.failures += 1;
            }
        }
    }
}

fn parse_contributions(plugin_id: &str, value: Value) -> Contributions {
    let mut c: Contributions = value
        .get("contributions")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    // Stamp ownership so the UI and dispatcher can route back to the plugin.
    for s in &mut c.metadata_sources {
        s.plugin_id = plugin_id.to_string();
    }
    for f in c.importers.iter_mut().chain(c.exporters.iter_mut()) {
        f.plugin_id = plugin_id.to_string();
    }
    for a in &mut c.item_actions {
        a.plugin_id = plugin_id.to_string();
    }
    for b in &mut c.badges {
        b.plugin_id = plugin_id.to_string();
    }
    c
}

#[async_trait]
impl PluginHost for PluginRegistry {
    async fn list(&self) -> Vec<PluginStatus> {
        let entries = self.entries.read().await;
        let mut out: Vec<PluginStatus> = entries.values().map(Entry::status).collect();
        out.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        out
    }

    async fn get(&self, id: &str) -> Result<PluginStatus> {
        self.entries
            .read()
            .await
            .get(id)
            .map(Entry::status)
            .ok_or_else(|| Error::not_found(format!("plugin {id}")))
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginStatus> {
        {
            let entries = self.entries.read().await;
            if !entries.contains_key(id) {
                return Err(Error::not_found(format!("plugin {id}")));
            }
        }
        if enabled {
            self.disabled.write().await.remove(id);
            self.set_state(id, PluginState::Stopped).await;
            self.start(id).await?;
        } else {
            self.disabled.write().await.insert(id.to_string());
            self.stop(id).await;
            self.set_state(id, PluginState::Disabled).await;
        }
        self.get(id).await
    }

    async fn reload(&self) -> Result<()> {
        let ids: Vec<String> = self.entries.read().await.keys().cloned().collect();
        for id in ids {
            self.stop(&id).await;
        }
        self.load().await
    }

    async fn contributions(&self) -> Contributions {
        let entries = self.entries.read().await;
        let mut all = Contributions::default();
        for e in entries.values().filter(|e| e.state.is_ready()) {
            all.merge(e.contributions.clone());
        }
        all
    }

    async fn call(&self, plugin_id: &str, method: &str, params: Value) -> Result<Value> {
        let (instance, timeout) = self.instance_of(plugin_id).await?;
        let started = Instant::now();
        let out = instance.call(method, params, timeout).await;
        self.record(plugin_id, started.elapsed(), out.is_ok()).await;
        out
    }

    async fn dispatch(&self, event: HookEvent) -> Vec<HookOutcome> {
        let targets: Vec<(String, Arc<Instance>, Duration)> = {
            let entries = self.entries.read().await;
            entries
                .values()
                .filter(|e| e.state.is_ready() && e.manifest.hooks.contains(&event.name))
                .filter_map(|e| {
                    e.instance.clone().map(|i| (e.manifest.id.clone(), i, e.timeout()))
                })
                .collect()
        };
        if targets.is_empty() {
            return Vec::new();
        }

        // Fan out concurrently: one slow subscriber must not delay the rest.
        let payload = json!({ "name": event.name, "payload": event.payload });
        let mut handles = Vec::with_capacity(targets.len());
        for (id, instance, timeout) in targets {
            let payload = payload.clone();
            handles.push(tokio::spawn(async move {
                let started = Instant::now();
                let result = instance.call("hook", payload, timeout).await;
                (id, result, started.elapsed())
            }));
        }

        let mut outcomes = Vec::with_capacity(handles.len());
        for h in handles {
            let Ok((id, result, elapsed)) = h.await else { continue };
            self.record(&id, elapsed, result.is_ok()).await;
            outcomes.push(match result {
                Ok(result) => HookOutcome {
                    plugin_id: id,
                    result,
                    duration_ms: elapsed.as_millis() as u64,
                    error: None,
                },
                Err(e) => HookOutcome {
                    plugin_id: id,
                    result: Value::Null,
                    duration_ms: elapsed.as_millis() as u64,
                    error: Some(e.to_string()),
                },
            });
        }
        outcomes.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        outcomes
    }

    async fn shutdown(&self) {
        let ids: Vec<String> = self.entries.read().await.keys().cloned().collect();
        for id in ids {
            self.stop(&id).await;
        }
    }
}

/// Fluent construction so the server can wire directories and builtins without
/// the registry knowing anything about configuration.
#[derive(Default)]
pub struct PluginHostBuilder {
    dirs: Vec<PathBuf>,
    builtins: Vec<Arc<dyn BuiltinPlugin>>,
    disabled: Vec<String>,
}

impl PluginHostBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dirs.push(dir.into());
        self
    }

    pub fn builtin(mut self, plugin: Arc<dyn BuiltinPlugin>) -> Self {
        self.builtins.push(plugin);
        self
    }

    /// Ids the user previously turned off.
    pub fn disabled(mut self, ids: Vec<String>) -> Self {
        self.disabled = ids;
        self
    }

    pub async fn build(self, host_api: Arc<dyn HostApi>) -> Result<Arc<PluginRegistry>> {
        let registry = Arc::new(PluginRegistry {
            dirs: self.dirs,
            builtins: self.builtins,
            host_api,
            entries: RwLock::new(HashMap::new()),
            disabled: RwLock::new(self.disabled.into_iter().collect()),
            diagnostics: RwLock::new(Vec::new()),
        });
        registry.load().await?;
        Ok(registry)
    }
}
