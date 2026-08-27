//! Runtime configuration: file, then environment, then CLI flags.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_port() -> u16 {
    23130
}
/// Rows pulled per embedding pass. Bigger is *not* better: the worker shares
/// one SQLite write lock with the API.
fn default_embed_batch() -> u32 {
    256
}
fn default_embed_interval() -> u64 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// `None` uses the platform data directory.
    pub data_dir: Option<PathBuf>,
    /// Directory holding the built web workbench. `None` disables UI hosting.
    pub web_dir: Option<PathBuf>,
    /// Extra plugin directories, in addition to `<data_dir>/plugins`.
    pub plugin_dirs: Vec<PathBuf>,
    /// Port for the Zotero browser connector, conventionally 23119.
    ///
    /// `None` — the default — means no second listener. See `serve_connector`.
    pub connector_port: Option<u16>,
    /// Optional bearer token. When set, every `/api` call must present it.
    pub api_key: Option<String>,
    pub embeddings: Embeddings,
    pub agent: AgentConfig,
}

fn default_agent_timeout() -> u64 {
    120
}

/// The library Q&A agent.
///
/// Off unless an endpoint and a model are named: an agent that silently talks
/// to a service the user never configured would be a surprise, and a local-first
/// tool should never make a network call nobody asked for.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// An OpenAI-compatible base URL, e.g. `http://127.0.0.1:11434/v1`.
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    #[serde(default = "default_agent_timeout")]
    pub timeout_secs: u64,
    /// Let the assistant run shell commands in its workspace directory.
    ///
    /// Off by default, and the flag *is* the boundary: a command runs with
    /// whatever rights the server has. There is no allowlist because a
    /// convincing one cannot be written — `python -c` defeats any list of
    /// program names — and a boundary that looks stronger than it is would be
    /// worse than an honest switch.
    #[serde(default)]
    pub allow_commands: bool,
    /// How many tool calls one turn may make. See `yk_agent::MAX_STEPS`.
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
}

fn default_max_steps() -> usize {
    yk_agent::MAX_STEPS
}

impl AgentConfig {
    pub fn is_configured(&self) -> bool {
        self.endpoint.is_some() && self.model.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Embeddings {
    /// `local` (offline, always available) or `remote`.
    pub provider: String,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub dimensions: usize,
    #[serde(default = "default_embed_batch")]
    pub batch: u32,
    /// Rows per embedding pass; see [`default_embed_batch`].
    #[serde(default = "default_embed_interval")]
    pub interval_secs: u64,
}

impl Default for Embeddings {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            endpoint: None,
            model: None,
            api_key: None,
            dimensions: yk_ai::providers::LOCAL_DIM,
            batch: default_embed_batch(),
            interval_secs: default_embed_interval(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            data_dir: None,
            web_dir: None,
            plugin_dirs: Vec::new(),
            connector_port: None,
            api_key: None,
            embeddings: Embeddings::default(),
            agent: AgentConfig { timeout_secs: default_agent_timeout(), ..Default::default() },
        }
    }
}

impl Config {
    /// Load `config.toml` from the data directory if present, then let the
    /// environment win — that ordering is what container users expect.
    pub fn load(explicit_data_dir: Option<PathBuf>) -> Self {
        let data_dir = explicit_data_dir.or_else(env_path("YK_DATA_DIR"));
        let probe = data_dir.clone().unwrap_or_else(default_data_dir);
        let mut cfg: Config = std::fs::read_to_string(probe.join("config.toml"))
            .ok()
            .and_then(|raw| match toml::from_str(&raw) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!(error = %e, "ignoring malformed config.toml");
                    None
                }
            })
            .unwrap_or_default();

        if let Some(d) = data_dir {
            cfg.data_dir = Some(d);
        }
        if let Some(h) = std::env::var("YK_HOST").ok().filter(|s| !s.is_empty()) {
            cfg.host = h;
        }
        if let Some(p) = std::env::var("YK_PORT").ok().and_then(|s| s.parse().ok()) {
            cfg.port = p;
        }
        if let Some(w) = env_path("YK_WEB_DIR")() {
            cfg.web_dir = Some(w);
        }
        if let Some(k) = std::env::var("YK_API_KEY").ok().filter(|s| !s.is_empty()) {
            cfg.api_key = Some(k);
        }
        if let Some(e) = std::env::var("YK_EMBED_ENDPOINT").ok().filter(|s| !s.is_empty()) {
            cfg.embeddings.provider = "remote".into();
            cfg.embeddings.endpoint = Some(e);
        }
        if let Some(m) = std::env::var("YK_EMBED_MODEL").ok().filter(|s| !s.is_empty()) {
            cfg.embeddings.model = Some(m);
        }
        if let Some(k) = std::env::var("YK_EMBED_API_KEY").ok().filter(|s| !s.is_empty()) {
            cfg.embeddings.api_key = Some(k);
        }
        if let Some(d) = std::env::var("YK_EMBED_DIM").ok().and_then(|s| s.parse().ok()) {
            cfg.embeddings.dimensions = d;
        }
        if let Some(e) = std::env::var("YK_AGENT_ENDPOINT").ok().filter(|s| !s.is_empty()) {
            cfg.agent.endpoint = Some(e);
        }
        if let Some(m) = std::env::var("YK_AGENT_MODEL").ok().filter(|s| !s.is_empty()) {
            cfg.agent.model = Some(m);
        }
        if let Some(k) = std::env::var("YK_AGENT_API_KEY").ok().filter(|s| !s.is_empty()) {
            cfg.agent.api_key = Some(k);
        }
        // Anything but an explicit "true" leaves it off: a capability this
        // broad should never be turned on by a typo.
        if let Ok(v) = std::env::var("YK_AGENT_ALLOW_COMMANDS") {
            cfg.agent.allow_commands = v.eq_ignore_ascii_case("true") || v == "1";
        }
        cfg
    }

    /// Write the configuration back to `config.toml`.
    ///
    /// So that pointing the assistant at a model is something the workbench
    /// can do. The program is a local server the user started; telling them to
    /// edit a file and restart it would make the web interface a partial one.
    ///
    /// Written to a temporary file and renamed, because a half-written
    /// `config.toml` is one the next start refuses to parse — and the failure
    /// would arrive long after the write.
    pub fn save(&self) -> yk_core::Result<()> {
        let dir = self.data_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| yk_core::Error::internal(format!("{}: {e}", dir.display())))?;

        let text = toml::to_string_pretty(self)
            .map_err(|e| yk_core::Error::internal(format!("serialising config: {e}")))?;
        let target = dir.join("config.toml");
        let temp = dir.join("config.toml.tmp");
        std::fs::write(&temp, text)
            .map_err(|e| yk_core::Error::internal(format!("{}: {e}", temp.display())))?;
        std::fs::rename(&temp, &target)
            .map_err(|e| yk_core::Error::internal(format!("{}: {e}", target.display())))?;
        Ok(())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(default_data_dir)
    }

    /// Where the assistant's skills live.
    ///
    /// Under the data directory so that adding one is copying a folder, and
    /// so that a user's skills survive an upgrade.
    pub fn skills_dir(&self) -> PathBuf {
        self.data_dir().join("skills")
    }

    /// The one directory the assistant may read, write and run things in.
    pub fn workspace_dir(&self) -> PathBuf {
        self.data_dir().join("workspace")
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir().join("yinkote.db")
    }

    /// Where attachment bytes live. Beside the database, so moving a library
    /// means moving one directory.
    pub fn storage_dir(&self) -> PathBuf {
        self.data_dir().join("storage")
    }

    /// Built-in plugin directory plus any configured extras.
    pub fn all_plugin_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.data_dir().join("plugins")];
        dirs.extend(self.plugin_dirs.iter().cloned());
        dirs
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn env_path(key: &'static str) -> impl Fn() -> Option<PathBuf> {
    move || std::env::var(key).ok().filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// `$XDG_DATA_HOME/yinkote` on Linux, `%APPDATA%\Yinkote` on Windows,
/// `~/Library/Application Support/Yinkote` on macOS.
pub fn default_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("yinkote");
        }
    }
    if cfg!(windows) {
        if let Ok(dir) = std::env::var("APPDATA") {
            return PathBuf::from(dir).join("Yinkote");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    if cfg!(target_os = "macos") {
        PathBuf::from(home).join("Library/Application Support/Yinkote")
    } else {
        PathBuf::from(home).join(".local/share/yinkote")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_bind_to_loopback_only() {
        let c = Config::default();
        assert_eq!(c.host, "127.0.0.1", "never expose the library by accident");
        assert_eq!(c.bind_addr(), "127.0.0.1:23130");
    }

    #[test]
    fn plugin_dirs_always_include_the_data_directory() {
        let c = Config { data_dir: Some(PathBuf::from("/tmp/yk")), ..Default::default() };
        assert_eq!(c.all_plugin_dirs()[0], PathBuf::from("/tmp/yk/plugins"));
    }

    #[test]
    fn parses_a_toml_config() {
        let c: Config = toml::from_str(
            r#"
            port = 9999
            [embeddings]
            provider = "remote"
            model = "text-embedding-3-small"
            dimensions = 1536
            "#,
        )
        .unwrap();
        assert_eq!(c.port, 9999);
        assert_eq!(c.host, "127.0.0.1", "missing keys fall back to defaults");
        assert_eq!(c.embeddings.dimensions, 1536);
    }
}
