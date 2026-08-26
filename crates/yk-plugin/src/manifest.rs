//! Plugin discovery and manifest validation.
//!
//! A plugin is a directory containing `plugin.json` (or `plugin.toml`). Nothing
//! else is required, which keeps the barrier to writing one very low.

use std::path::{Path, PathBuf};

use yk_core::plugin::{hooks, PluginManifest, PluginRuntime, PLUGIN_API_VERSION};
use yk_core::{Error, Result};

pub const MANIFEST_NAMES: [&str; 2] = ["plugin.json", "plugin.toml"];

/// A manifest plus where it came from.
#[derive(Debug, Clone)]
pub struct Discovered {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    /// Human-readable origin shown in the UI.
    pub source: String,
}

/// Scan every configured directory for plugins. Broken manifests are reported
/// but never abort discovery — one bad plugin must not hide the others.
pub fn discover(dirs: &[PathBuf]) -> (Vec<Discovered>, Vec<String>) {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match load_dir(&path) {
                Ok(Some(d)) => {
                    if seen.insert(d.manifest.id.clone()) {
                        found.push(d);
                    } else {
                        errors.push(format!(
                            "duplicate plugin id '{}' at {}",
                            d.manifest.id,
                            path.display()
                        ));
                    }
                }
                Ok(None) => {}
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
    }
    (found, errors)
}

fn load_dir(dir: &Path) -> Result<Option<Discovered>> {
    for name in MANIFEST_NAMES {
        let file = dir.join(name);
        if !file.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&file)?;
        let manifest: PluginManifest = if name.ends_with(".toml") {
            toml::from_str(&raw).map_err(|e| Error::plugin(format!("{name}: {e}")))?
        } else {
            serde_json::from_str(&raw).map_err(|e| Error::plugin(format!("{name}: {e}")))?
        };
        validate(&manifest)?;
        return Ok(Some(Discovered {
            manifest,
            dir: dir.to_path_buf(),
            source: dir.display().to_string(),
        }));
    }
    Ok(None)
}

pub fn validate(m: &PluginManifest) -> Result<()> {
    if m.id.is_empty()
        || !m
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(Error::plugin(format!(
            "invalid plugin id '{}': use [A-Za-z0-9._-]",
            m.id
        )));
    }
    if m.name.trim().is_empty() {
        return Err(Error::plugin("plugin name must not be empty"));
    }
    if m.api_version != PLUGIN_API_VERSION {
        return Err(Error::plugin(format!(
            "plugin '{}' targets API v{} but this host speaks v{PLUGIN_API_VERSION}",
            m.id, m.api_version
        )));
    }
    if let PluginRuntime::Process { command, .. } = &m.runtime {
        if command.trim().is_empty() {
            return Err(Error::plugin("process runtime requires a command"));
        }
    }
    for hook in &m.hooks {
        if !hooks::ALL.contains(&hook.as_str()) {
            return Err(Error::plugin(format!(
                "plugin '{}' subscribes to unknown hook '{hook}'",
                m.id
            )));
        }
    }
    if m.timeout_ms == 0 || m.timeout_ms > 600_000 {
        return Err(Error::plugin("timeoutMs must be between 1 and 600000"));
    }
    Ok(())
}

/// Resolve a manifest command against the plugin directory so plugins can ship
/// their own entry point without absolute paths.
pub fn resolve_command(dir: &Path, command: &str) -> String {
    let candidate = dir.join(command);
    if candidate.exists() {
        candidate.to_string_lossy().into_owned()
    } else {
        command.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn base() -> PluginManifest {
        PluginManifest {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            homepage: None,
            api_version: PLUGIN_API_VERSION,
            runtime: PluginRuntime::Process {
                command: "node".into(),
                args: vec!["main.js".into()],
                env: BTreeMap::new(),
            },
            capabilities: vec![],
            permissions: vec![],
            hooks: vec![],
            enabled: true,
            timeout_ms: 15_000,
        }
    }

    #[test]
    fn accepts_a_well_formed_manifest() {
        assert!(validate(&base()).is_ok());
    }

    #[test]
    fn rejects_bad_ids() {
        let mut m = base();
        m.id = "not valid!".into();
        assert!(validate(&m).is_err());
        m.id = String::new();
        assert!(validate(&m).is_err());
    }

    #[test]
    fn rejects_api_version_mismatch() {
        let mut m = base();
        m.api_version = 99;
        assert!(validate(&m).is_err());
    }

    #[test]
    fn rejects_unknown_hooks() {
        let mut m = base();
        m.hooks = vec!["item.explodes".into()];
        assert!(validate(&m).is_err());
    }

    #[test]
    fn rejects_empty_command() {
        let mut m = base();
        m.runtime = PluginRuntime::Process {
            command: "  ".into(),
            args: vec![],
            env: BTreeMap::new(),
        };
        assert!(validate(&m).is_err());
    }

    #[test]
    fn discovers_manifests_and_reports_broken_ones() {
        let tmp = std::env::temp_dir().join(format!("yk-plugtest-{}", std::process::id()));
        let good = tmp.join("good");
        let bad = tmp.join("bad");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(
            good.join("plugin.json"),
            serde_json::to_string(&base()).unwrap(),
        )
        .unwrap();
        std::fs::write(bad.join("plugin.json"), "{ not json").unwrap();

        let (found, errors) = discover(std::slice::from_ref(&tmp));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "demo");
        assert_eq!(errors.len(), 1);

        std::fs::remove_dir_all(&tmp).ok();
    }
}
