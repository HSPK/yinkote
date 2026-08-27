//! A workspace the agent can read, write and work in.
//!
//! Two capabilities the library tools cannot cover: keeping notes and scratch
//! files across a conversation, and running something. A screening pass over
//! four hundred abstracts wants a file to write results into; converting a
//! bibliography wants a program to do it.
//!
//! Both are scoped to one directory. Not because a sandbox makes arbitrary
//! code safe — it does not — but because "the agent can touch this folder" is
//! a sentence a user can hold in their head, while "the agent can touch your
//! home directory except…" is not. Running commands is off unless the config
//! turns it on, and the flag is the boundary: there is no allowlist pretending
//! to be one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use yk_ai::{Tool, ToolSpec};
use yk_core::{Error, Result};

/// Enough for a note or a result set; short of loading a corpus into context.
const MAX_READ: usize = 64 * 1024;
/// Command output beyond this is cut. A build log is not an answer.
const MAX_OUTPUT: usize = 16 * 1024;
/// A command that has not finished by now is not going to help this turn.
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The directory the agent works in.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Anchor a workspace at `root`, creating it if needed.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|e| Error::internal(format!("{}: {e}", root.display())))?;
        let root = root
            .canonicalize()
            .map_err(|e| Error::internal(format!("{}: {e}", root.display())))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Turn a relative path from the model into a real one inside the root.
    ///
    /// `..` is refused outright rather than resolved. Every path a caller
    /// legitimately wants can be written without it, and reasoning about a
    /// path that mixes `..` with symlinks is where this kind of check goes
    /// wrong: `notes/../../../secrets` walked out of an earlier version of
    /// this function because `Path::parent` strips `..` lexically instead of
    /// climbing.
    ///
    /// What remains is symlinks, which no amount of string handling catches,
    /// so the deepest existing ancestor is resolved and checked.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf> {
        use std::path::Component;

        let candidate = Path::new(relative);
        let mut clean = PathBuf::new();
        for part in candidate.components() {
            match part {
                Component::Normal(p) => clean.push(p),
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(Error::invalid("paths in the workspace cannot contain '..'"))
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(Error::invalid("give a path relative to the workspace"))
                }
            }
        }

        let joined = self.root.join(&clean);

        // A file being written does not exist yet, so check the deepest part
        // that does. Without `..` in the way, this is a real climb.
        let mut existing = joined.as_path();
        while !existing.exists() {
            match existing.parent() {
                Some(up) => existing = up,
                None => return Err(Error::invalid("that path leaves the workspace")),
            }
        }
        let real = existing
            .canonicalize()
            .map_err(|e| Error::invalid(format!("{relative}: {e}")))?;
        if !real.starts_with(&self.root) {
            return Err(Error::invalid("that path leaves the workspace"));
        }

        // When the target itself exists, the resolved form is the one to use.
        if existing == joined {
            return Ok(real);
        }
        Ok(joined)
    }

    /// The path as the model should see it: relative to the root.
    fn show(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).display().to_string()
    }
}

/// Cut a string on a character boundary, saying that it was cut.
fn clip(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => (text[..cut].to_string(), true),
        None => (text.to_string(), false),
    }
}

pub struct ListFiles {
    pub workspace: Workspace,
}

#[async_trait]
impl Tool for ListFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_files".into(),
            description: "List the files in your workspace directory.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "A folder inside the workspace. Defaults to the root.",
                    },
                },
            }),
        }
    }

    async fn call(&self, _library_id: i64, arguments: Value) -> Result<Value> {
        let relative = arguments["path"].as_str().unwrap_or(".");
        let dir = self.workspace.resolve(relative)?;
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir)
            .map_err(|e| Error::invalid(format!("{relative}: {e}")))?
            .flatten()
        {
            let meta = entry.metadata().ok();
            entries.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "path": self.workspace.show(&entry.path()),
                "directory": meta.as_ref().is_some_and(|m| m.is_dir()),
                "bytes": meta.as_ref().map(|m| m.len()).unwrap_or_default(),
            }));
        }
        entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(json!({ "path": self.workspace.show(&dir), "entries": entries }))
    }
}

pub struct ReadFile {
    pub workspace: Workspace,
}

#[async_trait]
impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a text file from your workspace directory.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        }
    }

    async fn call(&self, _library_id: i64, arguments: Value) -> Result<Value> {
        let relative = yk_agent::required_str(&arguments, "path")?;
        let path = self.workspace.resolve(&relative)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::invalid(format!("{relative}: {e}")))?;
        let (content, truncated) = clip(&text, MAX_READ);
        Ok(json!({ "path": relative, "content": content, "truncated": truncated }))
    }
}

pub struct WriteFile {
    pub workspace: Workspace,
}

#[async_trait]
impl Tool for WriteFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".into(),
            description: "Write a text file in your workspace directory, replacing whatever was \
                 there. Use it for notes, drafts and results you want to keep."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "append": {
                        "type": "boolean",
                        "description": "Add to the end instead of replacing.",
                    },
                },
                "required": ["path", "content"],
            }),
        }
    }

    async fn call(&self, _library_id: i64, arguments: Value) -> Result<Value> {
        let relative = yk_agent::required_str(&arguments, "path")?;
        let content = arguments["content"].as_str().unwrap_or_default();
        let path = self.workspace.resolve(&relative)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::invalid(format!("{relative}: {e}")))?;
        }

        if arguments["append"].as_bool().unwrap_or(false) {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| Error::invalid(format!("{relative}: {e}")))?;
            file.write_all(content.as_bytes())
                .map_err(|e| Error::invalid(format!("{relative}: {e}")))?;
        } else {
            std::fs::write(&path, content)
                .map_err(|e| Error::invalid(format!("{relative}: {e}")))?;
        }
        Ok(json!({ "path": relative, "bytes": content.len() }))
    }
}

/// Runs a shell command in the workspace.
///
/// Only registered when the config says so. There is no allowlist because a
/// convincing one cannot be written — `python -c` alone defeats any list of
/// program names — and a boundary that looks stronger than it is would be
/// worse than an honest switch.
pub struct RunCommand {
    pub workspace: Workspace,
}

#[async_trait]
impl Tool for RunCommand {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run_command".into(),
            description: format!(
                "Run a shell command in your workspace directory. It is cut off after {}s, and \
                 its output is truncated. Say what you ran and why.",
                COMMAND_TIMEOUT.as_secs()
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command line to run." },
                },
                "required": ["command"],
            }),
        }
    }

    async fn call(&self, _library_id: i64, arguments: Value) -> Result<Value> {
        let command = yk_agent::required_str(&arguments, "command")?;

        let mut child = tokio::process::Command::new(shell());
        child
            .arg(shell_flag())
            .arg(&command)
            .current_dir(self.workspace.root())
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null());

        let run = child.output();
        let output = match tokio::time::timeout(COMMAND_TIMEOUT, run).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(Error::invalid(format!("could not run it: {e}"))),
            // Reported rather than raised: a command that took too long is a
            // fact about the command, and the model can decide what to do.
            Err(_) => {
                return Ok(json!({
                    "command": command,
                    "timedOut": true,
                    "exitCode": null,
                    "stdout": "",
                    "stderr": format!("still running after {}s", COMMAND_TIMEOUT.as_secs()),
                }))
            }
        };

        let (stdout, out_cut) = clip(&String::from_utf8_lossy(&output.stdout), MAX_OUTPUT);
        let (stderr, err_cut) = clip(&String::from_utf8_lossy(&output.stderr), MAX_OUTPUT);
        Ok(json!({
            "command": command,
            "exitCode": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "truncated": out_cut || err_cut,
            "timedOut": false,
        }))
    }
}

fn shell() -> &'static str {
    if cfg!(windows) {
        "cmd"
    } else {
        "sh"
    }
}

fn shell_flag() -> &'static str {
    if cfg!(windows) {
        "/C"
    } else {
        "-c"
    }
}

/// The workspace tools, with command execution only if it was asked for.
pub fn tools(workspace: &Workspace, allow_commands: bool) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(ListFiles { workspace: workspace.clone() }),
        Arc::new(ReadFile { workspace: workspace.clone() }),
        Arc::new(WriteFile { workspace: workspace.clone() }),
    ];
    if allow_commands {
        tools.push(Arc::new(RunCommand { workspace: workspace.clone() }));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path().join("workspace")).unwrap();
        (dir, ws)
    }

    #[test]
    fn a_relative_path_lands_inside() {
        let (_dir, ws) = workspace();
        let path = ws.resolve("notes/screening.md").unwrap();
        assert!(path.starts_with(ws.root()));
    }

    #[test]
    fn climbing_out_is_refused() {
        let (_dir, ws) = workspace();
        assert!(ws.resolve("../../etc/passwd").is_err());
        assert!(ws.resolve("notes/../../../secrets").is_err());
    }

    #[test]
    fn an_absolute_path_is_refused() {
        let (_dir, ws) = workspace();
        assert!(ws.resolve("/etc/passwd").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_pointing_out_is_refused() {
        let (dir, ws) = workspace();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, ws.root().join("link.txt")).unwrap();

        // Resolving the joined path is the only thing that catches this; a
        // string check on `..` would happily follow the link.
        assert!(ws.resolve("link.txt").is_err());
    }

    #[tokio::test]
    async fn writes_then_reads_its_own_file() {
        let (_dir, ws) = workspace();
        WriteFile { workspace: ws.clone() }
            .call(1, json!({ "path": "notes/a.md", "content": "hello" }))
            .await
            .unwrap();
        let out = ReadFile { workspace: ws.clone() }
            .call(1, json!({ "path": "notes/a.md" }))
            .await
            .unwrap();
        assert_eq!(out["content"], "hello");
    }

    #[tokio::test]
    async fn appending_adds_rather_than_replaces() {
        let (_dir, ws) = workspace();
        let write = WriteFile { workspace: ws.clone() };
        write.call(1, json!({ "path": "log.txt", "content": "one\n" })).await.unwrap();
        write
            .call(1, json!({ "path": "log.txt", "content": "two\n", "append": true }))
            .await
            .unwrap();
        let out =
            ReadFile { workspace: ws }.call(1, json!({ "path": "log.txt" })).await.unwrap();
        assert_eq!(out["content"], "one\ntwo\n");
    }

    #[tokio::test]
    async fn listing_shows_what_is_there() {
        let (_dir, ws) = workspace();
        WriteFile { workspace: ws.clone() }
            .call(1, json!({ "path": "a.txt", "content": "x" }))
            .await
            .unwrap();
        let out = ListFiles { workspace: ws }.call(1, json!({})).await.unwrap();
        assert_eq!(out["entries"][0]["name"], "a.txt");
    }

    #[tokio::test]
    async fn writing_outside_the_workspace_fails() {
        let (_dir, ws) = workspace();
        let err = WriteFile { workspace: ws }
            .call(1, json!({ "path": "../escaped.txt", "content": "x" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("workspace"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_command_runs_in_the_workspace() {
        let (_dir, ws) = workspace();
        let out = RunCommand { workspace: ws.clone() }
            .call(1, json!({ "command": "printf hello > out.txt; printf done" }))
            .await
            .unwrap();
        assert_eq!(out["exitCode"], 0);
        assert_eq!(out["stdout"], "done");
        assert_eq!(std::fs::read_to_string(ws.root().join("out.txt")).unwrap(), "hello");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_failing_command_reports_rather_than_raises() {
        let (_dir, ws) = workspace();
        // The model can read an exit code and try something else; an error
        // would end the turn on a command that simply did not work.
        let out = RunCommand { workspace: ws }
            .call(1, json!({ "command": "exit 3" }))
            .await
            .unwrap();
        assert_eq!(out["exitCode"], 3);
    }

    #[test]
    fn commands_are_absent_unless_they_were_asked_for() {
        let (_dir, ws) = workspace();
        let names = |allow| {
            tools(&ws, allow).iter().map(|t| t.spec().name).collect::<Vec<_>>()
        };
        assert!(!names(false).contains(&"run_command".to_string()));
        assert!(names(true).contains(&"run_command".to_string()));
    }
}
