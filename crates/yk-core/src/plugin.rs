//! Plugin system contracts.
//!
//! These types are the stable boundary between the host and third-party code.
//! `yk-plugin` implements the runtime; the server only ever talks to the
//! [`crate::ports::PluginHost`] trait, so runtimes stay swappable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when the host↔plugin protocol changes incompatibly.
pub const PLUGIN_API_VERSION: u32 = 1;

/// How the host executes a plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginRuntime {
    /// A child process speaking JSON-RPC 2.0 over stdio. Language agnostic.
    Process {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Compiled into the host binary.
    Builtin,
}

/// What a plugin is allowed to do. Enforced by the host, not the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ItemsRead,
    ItemsWrite,
    CollectionsRead,
    CollectionsWrite,
    Search,
    Network,
    Settings,
    Notify,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Permission::ItemsRead => "read items",
            Permission::ItemsWrite => "create and modify items",
            Permission::CollectionsRead => "read collections",
            Permission::CollectionsWrite => "modify collections",
            Permission::Search => "run searches",
            Permission::Network => "access the network",
            Permission::Settings => "read and write its own settings",
            Permission::Notify => "show notifications",
        }
    }
}

/// Broad category of what a plugin contributes. Declared statically so the UI
/// can describe a plugin before ever starting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    /// Looks up bibliographic metadata in an external service.
    MetadataSource,
    /// Parses a file format into item drafts.
    Importer,
    /// Serialises items into a file format.
    Exporter,
    /// Contributes extra search results.
    SearchProvider,
    /// Adds a context action on items.
    ItemAction,
    /// Reacts to lifecycle hooks.
    Hook,
    /// Annotates items with a small value shown as a table column.
    Badge,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(rename = "apiVersion", default = "default_api_version")]
    pub api_version: u32,
    pub runtime: PluginRuntime,
    #[serde(default)]
    pub capabilities: Vec<CapabilityKind>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Hook names this plugin subscribes to (see [`hooks`]).
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Milliseconds before a call is aborted.
    #[serde(rename = "timeoutMs", default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_api_version() -> u32 {
    PLUGIN_API_VERSION
}
fn default_true() -> bool {
    true
}
fn default_timeout() -> u64 {
    15_000
}

/// Well-known hook names.
pub mod hooks {
    pub const STARTUP: &str = "startup";
    pub const SHUTDOWN: &str = "shutdown";
    pub const ITEM_BEFORE_CREATE: &str = "item.beforeCreate";
    pub const ITEM_CREATED: &str = "item.created";
    pub const ITEM_UPDATED: &str = "item.updated";
    pub const ITEM_TRASHED: &str = "item.trashed";
    pub const SEARCH_RERANK: &str = "search.rerank";

    pub const ALL: &[&str] = &[
        STARTUP,
        SHUTDOWN,
        ITEM_BEFORE_CREATE,
        ITEM_CREATED,
        ITEM_UPDATED,
        ITEM_TRASHED,
        SEARCH_RERANK,
    ];
}

/// Runtime registration returned by a plugin during `initialize`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Contributions {
    #[serde(default, rename = "metadataSources")]
    pub metadata_sources: Vec<SourceDescriptor>,
    #[serde(default)]
    pub importers: Vec<FormatDescriptor>,
    #[serde(default)]
    pub exporters: Vec<FormatDescriptor>,
    #[serde(default, rename = "itemActions")]
    pub item_actions: Vec<ActionDescriptor>,
    #[serde(default)]
    pub badges: Vec<BadgeDescriptor>,
}

impl Contributions {
    pub fn merge(&mut self, other: Contributions) {
        self.metadata_sources.extend(other.metadata_sources);
        self.importers.extend(other.importers);
        self.exporters.extend(other.exporters);
        self.item_actions.extend(other.item_actions);
        self.badges.extend(other.badges);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceDescriptor {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    /// e.g. `["query", "doi", "arxiv", "isbn"]`
    #[serde(default)]
    pub supports: Vec<String>,
    #[serde(rename = "pluginId", skip_deserializing, default)]
    pub plugin_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormatDescriptor {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(rename = "mimeType", default)]
    pub mime_type: Option<String>,
    #[serde(rename = "pluginId", skip_deserializing, default)]
    pub plugin_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionDescriptor {
    pub id: String,
    pub label: String,
    #[serde(rename = "itemTypes", default)]
    pub item_types: Vec<String>,
    #[serde(rename = "pluginId", skip_deserializing, default)]
    pub plugin_id: String,
}

/// A small per-item annotation shown as a table column.
///
/// Journal metrics (impact factor, JCR quartile, CAS tier) are the motivating
/// case: they are per-item, they come from datasets the host has no business
/// bundling, and they change independently of the library. Declaring them makes
/// the column exist; resolving them fills it in.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BadgeDescriptor {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Item fields the plugin needs in order to resolve, e.g. `["ISSN"]`.
    /// The host sends only these, so a badge plugin never sees the whole item.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Preferred column width in pixels.
    #[serde(default)]
    pub width: Option<u32>,
    /// Whether the column can be ordered by. Requires the plugin to return a
    /// `rank` with each value: sorting the *text* would put "10.5" before
    /// "9.8" and Q10 before Q2, which is worse than not offering it.
    #[serde(default)]
    pub sortable: bool,
    #[serde(rename = "pluginId", skip_deserializing, default)]
    pub plugin_id: String,
}

/// One resolved badge for one item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BadgeValue {
    /// Matches `BadgeDescriptor::id`.
    pub badge: String,
    pub text: String,
    /// How this value ranks against others in the same column. Higher sorts
    /// first when descending. The plugin decides what "higher" means, because
    /// only it knows whether tier 1 beats tier 4 or the reverse.
    #[serde(default)]
    pub rank: Option<f64>,
    /// Colour for this value. Either a severity (`high`, `mid`, `low`,
    /// `neutral`) or one of the collection palette names, so a plugin can give
    /// each level its own colour rather than three shades of one.
    #[serde(default)]
    pub tone: Option<String>,
    /// Longer text for a tooltip.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "pluginId", skip_deserializing, default)]
    pub plugin_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum PluginState {
    /// Manifest read, not started (either disabled or lazily loaded).
    Stopped,
    Starting,
    Ready,
    Disabled,
    Failed { error: String },
}

impl PluginState {
    pub fn is_ready(&self) -> bool {
        matches!(self, PluginState::Ready)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginStatus {
    #[serde(flatten)]
    pub manifest: PluginManifest,
    #[serde(flatten)]
    pub state: PluginState,
    pub contributions: Contributions,
    pub calls: u64,
    pub failures: u64,
    #[serde(rename = "avgLatencyMs")]
    pub avg_latency_ms: f64,
    pub source: String,
}

/// An event delivered to every plugin subscribed to `name`.
#[derive(Clone, Debug, Serialize)]
pub struct HookEvent {
    pub name: String,
    pub payload: Value,
}

impl HookEvent {
    pub fn new(name: impl Into<String>, payload: Value) -> Self {
        Self { name: name.into(), payload }
    }
}

/// Result of one plugin handling one hook.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutcome {
    pub plugin_id: String,
    pub result: Value,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A query handed to a metadata source.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExternalQuery {
    #[serde(default)]
    pub text: String,
    /// Identifier lookup: `{"doi": "10.1/x"}`.
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}
