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

/// Share of the time the embedding worker may spend holding the write lock.
///
/// SQLite has exactly one writer, so this is a direct trade: the lower the
/// number, the longer a large import takes to become searchable, and the less
/// an interactive save ever waits. A quarter keeps a save inside a frame or two
/// while still draining a hundred thousand items in a few minutes.
const EMBED_DUTY: u32 = 4;

/// Never spin, and never disappear for so long that progress looks stalled.
const EMBED_MIN_PAUSE: Duration = Duration::from_millis(20);
const EMBED_MAX_PAUSE: Duration = Duration::from_secs(2);

/// Drains the embedding queue that the store fills on every item write.
///
/// Kept out of the request path so writes stay fast, and deliberately
/// *unhurried*: SQLite has one write lock, and a background job that grabs it
/// greedily makes the UI feel broken.
///
/// The pause is measured rather than fixed. A fixed one cannot be right on both
/// a laptop and a server, and it is wrong in the direction that matters: when
/// the machine is slow or the database is contended a pass takes longer, which
/// is exactly when backing off harder is called for. Sleeping in proportion to
/// the pass just taken is self-tuning and needs no signal from the API.
fn embedding_worker(app: App) {
    let batch = app.config.embeddings.batch;
    let idle = Duration::from_secs(app.config.embeddings.interval_secs.max(1));

    tokio::spawn(async move {
        loop {
            let started = std::time::Instant::now();
            match app.search().embed_pending(batch).await {
                Ok(0) => tokio::time::sleep(idle).await,
                Ok(n) => {
                    app.events().publish(DomainEvent::IndexProgress {
                        done: n as i64,
                        total: n as i64,
                    });
                    let pause = (started.elapsed() * (EMBED_DUTY - 1))
                        .clamp(EMBED_MIN_PAUSE, EMBED_MAX_PAUSE);
                    tokio::time::sleep(pause).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "embedding pass failed; backing off");
                    tokio::time::sleep(idle * 5).await;
                }
            }
        }
    });
}

/// How large the write-ahead log's *file* may get before a blocking checkpoint
/// is worth it. Small enough that resetting it is quick, large enough that an
/// ordinary editing session never reaches it and never pauses.
const WAL_TRUNCATE_BYTES: u64 = 64 * 1024 * 1024;

/// Keep the write-ahead log from growing without bound.
///
/// Under sustained load a passive checkpoint can never finish — it only
/// reclaims frames older than the oldest live reader, and a busy pool always
/// has one — so this escalates once the log is large. See `Db::checkpoint`.
fn checkpoint_worker(app: App) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.tick().await; // the first tick fires immediately
        loop {
            ticker.tick().await;
            match app.store().db().checkpoint(WAL_TRUNCATE_BYTES).await {
                Ok(bytes) => tracing::trace!(bytes, "wal checkpoint"),
                Err(e) => tracing::debug!(error = %e, "wal checkpoint skipped"),
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
