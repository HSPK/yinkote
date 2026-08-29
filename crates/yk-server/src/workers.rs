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
    download_worker(app.clone());
    keep_statistics_current(app.clone());
    warm_first_load(app.clone());
    startup_hook(app);
}

/// Give the query planner something to plan with.
///
/// SQLite chooses between materialising a filter and probing it from
/// `sqlite_stat1`. With no statistics it guesses, and one guess is expensive:
/// listing a shelf walks the sort index probing membership per row instead of
/// reading the 4,500 members and sorting them — 14.2ms against 4.4ms on a
/// 131,940-item library, measured both ways on the same file.
///
/// Nothing ran this except a hand-written POST to `/maintenance/optimize`, so
/// a real library never had statistics at all. The benchmark had been calling
/// it after seeding to be fair to the server, which quietly hid that every
/// user was in the state the benchmark refused to measure. See docs/16 §3.187.
///
/// `PRAGMA optimize` is built for exactly this: it analyses only what has
/// changed enough to matter, so the repeat is nearly free and the first run on
/// an already-analysed library does almost nothing.
fn keep_statistics_current(app: App) {
    tokio::spawn(async move {
        // Behind the listener. Statistics decide how fast queries are, not
        // whether they answer, so nobody should wait on this to open the page.
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            // Same restraint as `checkpoint_worker`, for the same reason: a
            // bulk write is already holding the database as much as it can,
            // and `ANALYZE` reads every index it looks at. The statistics of
            // a library mid-import are about to be wrong anyway.
            if !app.tasks().bulk_write_running() {
                // Statistics only. The checkpoint half of `maintenance` takes
                // the database exclusively and has a worker of its own that
                // knows when that is safe.
                if let Err(error) = app.store().db().refresh_statistics().await {
                    tracing::debug!(%error, "could not refresh planner statistics");
                }
                // And the full-text index's own housekeeping, which `PRAGMA
                // optimize` does not do despite the name: every write appends
                // a segment and nothing merged them, so search got slower for
                // as long as the library was used.
                if let Err(error) = app.store().db().merge_search_segments().await {
                    tracing::debug!(%error, "could not merge search segments");
                }
            }
            // Often enough to follow a big import, rare enough to be invisible.
            tokio::time::sleep(Duration::from_secs(60 * 30)).await;
        }
    });
}

/// Do the reads the first page load does, before anybody asks for them.
///
/// Two separate costs, both paid by whoever opens the workbench first.
///
/// The facet cache serves nothing stale when empty — with no previous answer
/// to show, correctness is the only option — so the first request pays the
/// full count: 227 ms on a hundred-thousand-item library against 0.7 ms warm.
///
/// The library statistics are not cached at all and still took 303 ms cold
/// against 9 ms after, which is not the query — it is the first touch of the
/// index pages, read from disk. Counting once here brings them into the page
/// cache.
///
/// In both cases the work happens either way; doing it before it is asked for
/// costs nothing and removes every perceptible pause from a cold start. What
/// it *must* get right is asking for exactly what the client asks for — a
/// warm-up that differs by one parameter fills a slot nobody requests, which
/// has now happened twice. See `docs/16` §3.48–3.49.
fn warm_first_load(app: App) {
    tokio::spawn(async move {
        // After the listener is up: a user who opens the page instantly
        // should not queue behind this, and the cache fills either way.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Built the way the route builds it, not from `ItemFilter::default()`.
        // The cache key includes every field, and the route defaults
        // `recursive` to *true* while the type defaults it to false — a warm
        // filter written by hand filled a slot nobody ever asks for, and the
        // first real request still paid the full count.
        let Ok(filter) = crate::routes::ListParams::default().filter(app.services.default_library)
        else {
            return;
        };
        if let Err(error) = app.store().tags.facets(&filter, crate::routes::FACET_LIMIT).await {
            tracing::debug!(%error, "could not warm the facet cache");
        }

        // The sidebar's collection list, which carries a live-item count per
        // shelf and costs 27ms cold on a large library — the slowest of the
        // requests the first paint waits on until it is remembered.
        let _ = app.store().collections.list(app.services.default_library).await;

        // The same two counts the statistics endpoint makes. The numbers are
        // thrown away; the point is the pages they touch.
        let trashed = yk_core::query::ItemFilter {
            trash: yk_core::query::TrashScope::Only,
            ..filter.clone()
        };
        let _ = app.store().items.count(&filter).await;
        let _ = app.store().items.count(&trashed).await;
        let _ = app.search().stats().await;

        tracing::debug!("first-load reads warmed");
    });
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
    let batch = app.config().embeddings.batch;
    let idle = Duration::from_secs(app.config().embeddings.interval_secs.max(1));

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
            // Not while anything is writing in bulk. A `TRUNCATE` checkpoint
            // takes the database exclusively, and a bulk write produces exactly
            // the large log that invites one — the two together stopped
            // interactive writes for the whole fifteen-second busy timeout
            // during a rebuild, and made them take seconds during an import.
            // The log is reclaimed when the job finishes; a big file for a few
            // minutes is cheaper than a program that will not accept a
            // keystroke.
            if app.tasks().bulk_write_running() {
                continue;
            }
            match app.store().db().checkpoint(WAL_TRUNCATE_BYTES).await {
                Ok(bytes) => tracing::trace!(bytes, "wal checkpoint"),
                Err(e) => tracing::debug!(error = %e, "wal checkpoint skipped"),
            }
        }
    });
}

/// How long to wait when the queue is empty.
///
/// Polling rather than a notification because the queue is a *table*: things
/// arrive in it from the workbench, from the browser connector and from the
/// agent, and a signal every one of them had to remember to send is a signal
/// one of them would forget.
const IDLE: Duration = Duration::from_secs(3);

/// Between downloads, so a hundred queued files do not become a hundred
/// simultaneous requests to one publisher.
const BETWEEN: Duration = Duration::from_millis(400);

/// Drains the download queue, one file at a time.
fn download_worker(app: App) {
    tokio::spawn(async move {
        loop {
            let claimed = match app.store().downloads.claim(app.services.default_library).await {
                Ok(job) => job,
                Err(e) => {
                    tracing::debug!(error = %e, "download queue unavailable");
                    tokio::time::sleep(IDLE).await;
                    continue;
                }
            };

            let Some(job) = claimed else {
                tokio::time::sleep(IDLE).await;
                continue;
            };

            let lib = app.services.default_library;
            let outcome = match yk_core::Key::parse(&job.item_key) {
                Ok(key) => {
                    crate::routes::files::attach_url(&app, lib, &key, &job.url, &job.title).await
                }
                Err(_) => Err(yk_core::Error::invalid("that is not an item key")),
            };

            match outcome {
                Ok((attachment, bytes)) => {
                    let _ = app.store().downloads.succeed(job.id, bytes).await;
                    let version =
                        app.store().libraries.version(lib).await.unwrap_or_default();
                    app.events().publish(DomainEvent::ItemsChanged {
                        library_id: lib,
                        keys: vec![attachment.key],
                        version,
                    });
                }
                Err(e) => {
                    // Recorded rather than logged: the reason is what the user
                    // needs in order to decide whether retrying is worth it.
                    let _ = app.store().downloads.fail(job.id, &e.detail()).await;
                }
            }

            tokio::time::sleep(BETWEEN).await;
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
