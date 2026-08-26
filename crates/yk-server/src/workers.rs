//! Background workers.

use std::time::Duration;

use yk_core::event::DomainEvent;
use yk_core::plugin::{hooks, HookEvent};

use crate::state::App;

/// Start every background task. All of them are best-effort: a failure logs and
/// retries rather than taking the server down.
pub fn spawn(app: App) {
    embedding_worker(app.clone());
    checkpoint_worker(app.clone());
    startup_hook(app);
}

/// Drains the embedding queue that the store fills on every item write.
///
/// Kept out of the request path so writes stay fast, and deliberately
/// *unhurried*: SQLite has one write lock, and a background job that grabs it
/// greedily makes the UI feel broken. It always yields between passes.
fn embedding_worker(app: App) {
    let batch = app.config.embeddings.batch;
    let idle = Duration::from_secs(app.config.embeddings.interval_secs.max(1));

    tokio::spawn(async move {
        loop {
            match app.search().embed_pending(batch).await {
                Ok(0) => tokio::time::sleep(idle).await,
                Ok(n) => {
                    app.events().publish(DomainEvent::IndexProgress {
                        done: n as i64,
                        total: n as i64,
                    });
                    // Deliberate breathing room for interactive writers.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "embedding pass failed; backing off");
                    tokio::time::sleep(idle * 5).await;
                }
            }
        }
    });
}

/// Under sustained write load the WAL never gets a quiet moment to checkpoint
/// itself and grows without bound. A passive checkpoint is a no-op when the
/// database is busy, so this is safe to run on a timer.
fn checkpoint_worker(app: App) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.tick().await; // the first tick fires immediately
        loop {
            ticker.tick().await;
            if let Err(e) = app.store().db().checkpoint().await {
                tracing::debug!(error = %e, "wal checkpoint skipped");
            }
        }
    });
}

fn startup_hook(app: App) {
    tokio::spawn(async move {
        let payload = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "defaultLibrary": app.services.default_library,
        });
        for outcome in app.plugins.dispatch(HookEvent::new(hooks::STARTUP, payload)).await {
            if let Some(err) = outcome.error {
                tracing::warn!(plugin = %outcome.plugin_id, "startup hook failed: {err}");
            }
        }
    });
}
