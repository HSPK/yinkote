//! End-to-end tests for the plugin runtime against a real child process.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use yk_core::plugin::*;
use yk_core::ports::{HostApi, PluginHost};
use yk_core::{Error, Result};

use yk_plugin::*;

/// Tests run in parallel, so every temporary path must be unique. Wall-clock
/// milliseconds are not: two tests routinely start in the same one.
fn unique_dir(prefix: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Records what plugins asked for and enforces declared permissions.
#[derive(Default)]
struct RecordingHost {
    calls: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl HostApi for RecordingHost {
    async fn invoke(
        &self,
        plugin_id: &str,
        granted: &[Permission],
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.calls.lock().push((plugin_id.to_string(), method.to_string()));
        let required = match method {
            "host.log" | "host.notify" => None,
            "host.items.search" => Some(Permission::Search),
            "host.items.get" => Some(Permission::ItemsRead),
            "host.items.create" => Some(Permission::ItemsWrite),
            _ => return Err(Error::not_found(format!("host method '{method}'"))),
        };
        if let Some(p) = required {
            if !granted.contains(&p) {
                return Err(Error::Forbidden(format!("plugin needs permission to {}", p.label())));
            }
        }
        Ok(json!({ "ok": true, "echo": params }))
    }
}

struct TestPlugin {
    dir: PathBuf,
}

impl TestPlugin {
    /// Materialise a plugin directory pointing at the mock binary.
    fn create(id: &str, permissions: Vec<Permission>, hooks: Vec<String>) -> Self {
        let dir = unique_dir(&format!("yk-plug-{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = PluginManifest {
            id: id.into(),
            name: format!("Test {id}"),
            version: "1.0.0".into(),
            description: Some("integration test plugin".into()),
            author: None,
            homepage: None,
            api_version: PLUGIN_API_VERSION,
            runtime: PluginRuntime::Process {
                command: env!("CARGO_BIN_EXE_yk-mock-plugin").to_string(),
                args: vec![],
                env: BTreeMap::new(),
            },
            capabilities: vec![CapabilityKind::MetadataSource, CapabilityKind::Hook],
            permissions,
            hooks,
            enabled: true,
            timeout_ms: 4_000,
        };
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        Self { dir }
    }
}

impl Drop for TestPlugin {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// A plugin root directory that removes itself when the test ends.
struct Root(PathBuf);

impl std::ops::Deref for Root {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Isolate each test in its own plugin root directory.
fn root_with(plugins: &[&TestPlugin]) -> Root {
    let root = unique_dir("yk-plugroot");
    std::fs::create_dir_all(&root).unwrap();
    for p in plugins {
        let dest = root.join(p.dir.file_name().unwrap());
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::copy(p.dir.join("plugin.json"), dest.join("plugin.json")).unwrap();
    }
    Root(root)
}

async fn registry_with(
    plugins: &[&TestPlugin],
    host: Arc<RecordingHost>,
) -> (Arc<PluginRegistry>, Root) {
    let root = root_with(plugins);
    let registry = PluginHostBuilder::new().dir(root.to_path_buf()).build(host).await.unwrap();
    (registry, root)
}

#[tokio::test]
async fn starts_plugin_and_collects_contributions() {
    let p = TestPlugin::create("alpha", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;

    let list = reg.list().await;
    assert_eq!(list.len(), 1);
    assert!(list[0].state.is_ready(), "state: {:?}", list[0].state);

    let c = reg.contributions().await;
    assert_eq!(c.metadata_sources.len(), 1);
    assert_eq!(c.metadata_sources[0].plugin_id, "alpha", "ownership is stamped");
    assert_eq!(c.importers[0].id, "mockfmt");
    reg.shutdown().await;
}

#[tokio::test]
async fn calls_round_trip() {
    let p = TestPlugin::create("beta", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;

    let out = reg.call("beta", "echo", json!({"n": 42})).await.unwrap();
    assert_eq!(out["n"], 42);

    let status = reg.get("beta").await.unwrap();
    assert!(status.calls >= 2, "initialize + echo counted");
    reg.shutdown().await;
}

#[tokio::test]
async fn plugin_errors_surface_without_killing_the_host() {
    let p = TestPlugin::create("gamma", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;

    let err = reg.call("gamma", "boom", Value::Null).await.unwrap_err();
    assert!(err.to_string().contains("intentional failure"), "{err}");
    // Still usable afterwards.
    assert!(reg.call("gamma", "echo", json!(1)).await.is_ok());
    assert_eq!(reg.get("gamma").await.unwrap().failures, 1);
    reg.shutdown().await;
}

#[tokio::test]
async fn unknown_method_is_reported() {
    let p = TestPlugin::create("delta", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;
    let err = reg.call("delta", "nope", Value::Null).await.unwrap_err();
    assert!(err.to_string().contains("unknown method"), "{err}");
    reg.shutdown().await;
}

#[tokio::test]
async fn slow_plugin_is_timed_out() {
    let p = TestPlugin::create("slowpoke", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;
    let started = std::time::Instant::now();
    let err = reg.call("slowpoke", "slow", Value::Null).await.unwrap_err();
    assert!(err.to_string().contains("timed out"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    reg.shutdown().await;
}

#[tokio::test]
async fn crashed_plugin_is_restarted_on_next_call() {
    let p = TestPlugin::create("crashy", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;

    assert!(reg.call("crashy", "crash", Value::Null).await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    // The registry transparently respawns it.
    let out = reg.call("crashy", "echo", json!("back")).await.unwrap();
    assert_eq!(out, json!("back"));
    reg.shutdown().await;
}

#[tokio::test]
async fn hooks_reach_only_subscribers() {
    let subscriber =
        TestPlugin::create("subscriber", vec![], vec![hooks::ITEM_CREATED.to_string()]);
    let bystander = TestPlugin::create("bystander", vec![], vec![]);
    let (reg, _root) = registry_with(&[&subscriber, &bystander], Arc::new(RecordingHost::default())).await;

    let outcomes = reg
        .dispatch(HookEvent::new(hooks::ITEM_CREATED, json!({"key": "ABCD1234"})))
        .await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].plugin_id, "subscriber");
    assert_eq!(outcomes[0].result["seen"], hooks::ITEM_CREATED);

    assert!(reg.dispatch(HookEvent::new(hooks::SHUTDOWN, Value::Null)).await.is_empty());
    reg.shutdown().await;
}

#[tokio::test]
async fn host_callbacks_are_permission_checked() {
    let host = Arc::new(RecordingHost::default());
    let allowed = TestPlugin::create("allowed", vec![Permission::ItemsRead], vec![]);
    let (reg, _root) = registry_with(&[&allowed], host.clone()).await;

    let ok = reg
        .call("allowed", "callhost", json!({"method": "host.items.get", "params": {"key":"X"}}))
        .await
        .unwrap();
    assert_eq!(ok["result"]["ok"], true);

    let denied = reg
        .call("allowed", "callhost", json!({"method": "host.items.create", "params": {}}))
        .await
        .unwrap();
    assert_eq!(denied["error"]["code"], codes_permission_denied());
    assert!(host.calls.lock().iter().any(|(_, m)| m == "host.items.create"));
    reg.shutdown().await;
}

fn codes_permission_denied() -> i64 {
    yk_plugin::rpc::codes::PERMISSION_DENIED
}

#[tokio::test]
async fn disable_and_enable_controls_the_process() {
    let p = TestPlugin::create("toggle", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;

    let off = reg.set_enabled("toggle", false).await.unwrap();
    assert_eq!(off.state, PluginState::Disabled);
    assert!(reg.contributions().await.metadata_sources.is_empty());
    let err = reg.call("toggle", "echo", json!(1)).await.unwrap_err();
    assert_eq!(err.kind(), yk_core::ErrorKind::Forbidden);

    let on = reg.set_enabled("toggle", true).await.unwrap();
    assert!(on.state.is_ready());
    assert!(reg.call("toggle", "echo", json!(1)).await.is_ok());
    reg.shutdown().await;
}

#[tokio::test]
async fn reload_picks_up_new_plugins() {
    let a = TestPlugin::create("one", vec![], vec![]);
    let root = root_with(&[&a]);
    let reg = PluginHostBuilder::new()
        .dir(root.to_path_buf())
        .build(Arc::new(RecordingHost::default()))
        .await
        .unwrap();
    assert_eq!(reg.list().await.len(), 1);

    let b = TestPlugin::create("two", vec![], vec![]);
    let dest = root.join("two");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(b.dir.join("plugin.json"), dest.join("plugin.json")).unwrap();

    reg.reload().await.unwrap();
    let ids: Vec<String> = reg.list().await.into_iter().map(|s| s.manifest.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"two".to_string()));
    reg.shutdown().await;
}

#[tokio::test]
async fn disabled_state_survives_reload() {
    let p = TestPlugin::create("sticky", vec![], vec![]);
    let (reg, _root) = registry_with(&[&p], Arc::new(RecordingHost::default())).await;
    reg.set_enabled("sticky", false).await.unwrap();
    reg.reload().await.unwrap();
    assert_eq!(reg.get("sticky").await.unwrap().state, PluginState::Disabled);
    reg.shutdown().await;
}

#[tokio::test]
async fn broken_manifest_is_reported_not_fatal() {
    let good = TestPlugin::create("good", vec![], vec![]);
    let root = root_with(&[&good]);
    let broken = root.join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.json"), "{{{ not json").unwrap();

    let reg = PluginHostBuilder::new()
        .dir(root.to_path_buf())
        .build(Arc::new(RecordingHost::default()))
        .await
        .unwrap();
    assert_eq!(reg.list().await.len(), 1);
    assert_eq!(reg.diagnostics().await.len(), 1);
    reg.shutdown().await;
}

#[tokio::test]
async fn missing_binary_marks_plugin_failed() {
    let root = Root(unique_dir("yk-plugbad"));
    let dir = root.join("ghost");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "id": "ghost", "name": "Ghost", "version": "1.0.0", "apiVersion": 1,
        "runtime": { "type": "process", "command": "definitely-not-a-real-binary-xyz" }
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();

    let reg = PluginHostBuilder::new()
        .dir(root.to_path_buf())
        .build(Arc::new(RecordingHost::default()))
        .await
        .unwrap();
    let s = reg.get("ghost").await.unwrap();
    assert!(matches!(s.state, PluginState::Failed { .. }), "{:?}", s.state);
}

struct Counter;

#[async_trait]
impl BuiltinPlugin for Counter {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "counter".into(),
            name: "Counter".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            homepage: None,
            api_version: PLUGIN_API_VERSION,
            runtime: PluginRuntime::Builtin,
            capabilities: vec![CapabilityKind::Hook],
            permissions: vec![],
            hooks: vec![hooks::ITEM_CREATED.into()],
            enabled: true,
            timeout_ms: 1000,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({"contributions": {}})),
            "hook" => Ok(json!({"counted": true, "name": params["name"]})),
            _ => Err(Error::not_found(method)),
        }
    }
}

#[tokio::test]
async fn builtin_plugins_share_the_same_surface() {
    let reg = PluginHostBuilder::new()
        .builtin(Arc::new(Counter))
        .build(Arc::new(RecordingHost::default()))
        .await
        .unwrap();

    assert!(reg.get("counter").await.unwrap().state.is_ready());
    let outcomes = reg.dispatch(HookEvent::new(hooks::ITEM_CREATED, json!({}))).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].result["counted"], true);
    reg.shutdown().await;
}
