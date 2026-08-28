//! Starting and stopping the browser connector while the server runs.
//!
//! The connector is off by default and for a good reason: port 23119 belongs
//! to Zotero, and taking it would break a copy the user may still be migrating
//! from. But "off by default" was paired with "the only way to turn it on is a
//! command-line flag", on a product whose recommended install is a background
//! service — so the switch existed somewhere the user never types.
//!
//! That mattered more once the scraper learned to say "this publisher refused
//! us; save it with the browser connector instead": advice pointing at a
//! feature the reader cannot reach is worse than no advice.
//!
//! So the listener is startable and stoppable at runtime. The interesting part
//! is not the binding, it is that **asking is not the same as succeeding** — a
//! running Zotero owns the port, and the caller has to be told that now, not
//! left to discover it the next time they save a page.

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::task::JoinHandle;

use crate::state::App;

/// The connector listener, as it actually is.
///
/// Holds the port that is *bound*, never the port that was asked for. The two
/// diverge exactly when it matters, and a status built from the request would
/// claim browser saving works at the moment it does not.
#[derive(Default)]
pub struct Connector {
    task: Option<JoinHandle<()>>,
    port: Option<u16>,
}

impl Connector {
    /// The port currently being served, if any.
    pub fn port(&self) -> Option<u16> {
        self.port
    }
}

/// A handle shared with the router so requests can toggle the listener.
pub type Shared = Arc<Mutex<Connector>>;

/// Bind `port` and serve the API on it, replacing any current listener.
///
/// Errors carry the bind failure, because "something else already has 23119"
/// is the whole answer and is almost always a running Zotero.
pub async fn start(app: &App, shared: &Shared, port: u16) -> Result<(), std::io::Error> {
    let addr = format!("127.0.0.1:{port}");
    // Bind before stopping the old one: if the new port is taken, the user is
    // left with the listener they already had rather than with nothing.
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    stop(shared);

    tracing::info!(%addr, "browser connector listening");
    let router = crate::router(app.clone());
    let task = tokio::spawn(async move {
        let served = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>());
        if let Err(e) = served.await {
            tracing::warn!(error = %e, "browser connector stopped");
        }
    });

    let mut guard = shared.lock();
    guard.task = Some(task);
    guard.port = Some(port);
    Ok(())
}

/// Stop serving the connector port. Safe to call when nothing is running.
pub fn stop(shared: &Shared) {
    let mut guard = shared.lock();
    if let Some(task) = guard.task.take() {
        task.abort();
        tracing::info!(port = guard.port, "browser connector stopped");
    }
    guard.port = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stopping when nothing runs must be silent, because that is the state a
    /// fresh server is in and `stop` is called on every reconfiguration.
    #[test]
    fn stopping_nothing_is_not_an_error() {
        let shared: Shared = Default::default();
        stop(&shared);
        assert_eq!(shared.lock().port(), None);
    }

    /// The port is recorded only once something is listening on it. A status
    /// built from the request would claim browser saving works at the exact
    /// moment a running Zotero has the port.
    #[test]
    fn a_fresh_connector_reports_no_port() {
        assert_eq!(Connector::default().port(), None);
    }
}
