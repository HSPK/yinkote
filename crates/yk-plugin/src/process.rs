//! Out-of-process plugin instances.
//!
//! A plugin is an ordinary child process that reads newline-delimited JSON-RPC
//! on stdin and writes it on stdout. That makes plugins writable in any
//! language and, crucially, isolates crashes and hangs from the host.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use yk_core::plugin::{Permission, PluginManifest, PluginRuntime};
use yk_core::ports::HostApi;
use yk_core::{Error, Result};

use crate::manifest::resolve_command;
use crate::rpc::{codes, Incoming, Request, Response};

type Pending = Arc<parking_lot::Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

pub struct PluginProcess {
    id: String,
    permissions: Vec<Permission>,
    outbox: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    child: Mutex<Child>,
}

impl PluginProcess {
    /// Spawn the child and wire up both directions of the protocol.
    pub async fn start(
        manifest: &PluginManifest,
        dir: &Path,
        host: Arc<dyn HostApi>,
    ) -> Result<Arc<Self>> {
        let PluginRuntime::Process { command, args, env } = &manifest.runtime else {
            return Err(Error::plugin("not a process plugin"));
        };

        let mut cmd = Command::new(resolve_command(dir, command));
        cmd.args(args)
            .current_dir(dir)
            .envs(env)
            .env("YK_PLUGIN_ID", &manifest.id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| Error::plugin(format!("cannot start '{}': {e}", manifest.id)))?;

        let stdin = child.stdin.take().ok_or_else(|| Error::plugin("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| Error::plugin("no stdout"))?;
        let stderr = child.stderr.take();

        let (outbox, mut rx) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Arc::new(parking_lot::Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));

        // Writer: single owner of stdin, so no interleaved lines.
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err()
                    || stdin.write_all(b"\n").await.is_err()
                    || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        });

        // Reader: routes responses to waiters and serves the plugin's own calls.
        {
            let pending = pending.clone();
            let alive = alive.clone();
            let outbox_for_reader = outbox.clone();
            let plugin_id = manifest.id.clone();
            let permissions = manifest.permissions.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let Ok(msg) = serde_json::from_str::<Incoming>(&line) else {
                        tracing::warn!(plugin = %plugin_id, "unparseable message from plugin");
                        continue;
                    };

                    if msg.is_request() {
                        let host = host.clone();
                        let outbox = outbox_for_reader.clone();
                        let plugin_id = plugin_id.clone();
                        let permissions = permissions.clone();
                        tokio::spawn(async move {
                            let id = msg.id.unwrap_or(0);
                            let method = msg.method.unwrap_or_default();
                            let params = msg.params.unwrap_or(Value::Null);
                            let reply = match host
                                .invoke(&plugin_id, &permissions, &method, params)
                                .await
                            {
                                Ok(v) => Response::ok(id, v),
                                Err(e) => Response::err(id, host_error_code(&e), e.to_string()),
                            };
                            if let Ok(s) = serde_json::to_string(&reply) {
                                let _ = outbox.send(s);
                            }
                        });
                        continue;
                    }

                    if let Some(id) = msg.id {
                        if let Some(tx) = pending.lock().remove(&id) {
                            let out = match (msg.result, msg.error) {
                                (_, Some(e)) => Err(Error::plugin(format!(
                                    "{} (code {})",
                                    e.message, e.code
                                ))),
                                (Some(v), None) => Ok(v),
                                (None, None) => Ok(Value::Null),
                            };
                            let _ = tx.send(out);
                        }
                    }
                }
                // The pipe closed: fail every waiter instead of hanging.
                alive.store(false, Ordering::SeqCst);
                for (_, tx) in pending.lock().drain() {
                    let _ = tx.send(Err(Error::plugin("plugin exited")));
                }
            });
        }

        if let Some(stderr) = stderr {
            let plugin_id = manifest.id.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::warn!(plugin = %plugin_id, "{line}");
                }
            });
        }

        Ok(Arc::new(Self {
            id: manifest.id.clone(),
            permissions: manifest.permissions.clone(),
            outbox,
            pending,
            next_id: AtomicU64::new(1),
            alive,
            child: Mutex::new(child),
        }))
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Send a request and await the matching response.
    pub async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        if !self.is_alive() {
            return Err(Error::plugin(format!("plugin '{}' is not running", self.id)));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);

        let line = serde_json::to_string(&Request::new(id, method, params))
            .map_err(|e| Error::plugin(format!("encode: {e}")))?;
        if self.outbox.send(line).is_err() {
            self.pending.lock().remove(&id);
            return Err(Error::plugin(format!("plugin '{}' stdin closed", self.id)));
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::plugin("plugin dropped the response")),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(Error::plugin(format!(
                    "plugin '{}' timed out after {:?} on '{method}'",
                    self.id, timeout
                )))
            }
        }
    }

    /// Ask politely, then insist.
    pub async fn stop(&self) {
        if self.is_alive() {
            let _ = self.call("shutdown", Value::Null, Duration::from_millis(1500)).await;
        }
        self.alive.store(false, Ordering::SeqCst);
        let _ = self.child.lock().await.kill().await;
    }
}

fn host_error_code(e: &Error) -> i64 {
    match e.kind() {
        yk_core::ErrorKind::Forbidden | yk_core::ErrorKind::Unauthorized => {
            codes::PERMISSION_DENIED
        }
        yk_core::ErrorKind::NotFound => codes::INVALID_PARAMS,
        yk_core::ErrorKind::Invalid => codes::INVALID_PARAMS,
        _ => codes::INTERNAL_ERROR,
    }
}
