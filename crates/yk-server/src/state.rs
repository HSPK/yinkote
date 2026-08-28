//! Shared application state.
//!
//! Split in two on purpose: [`Services`] is everything a *plugin* may reach
//! through the host API, while [`AppState`] additionally owns the plugin
//! registry. That ordering breaks what would otherwise be a construction cycle
//! (plugins need the host API, the app needs the plugins) without any
//! late-initialisation tricks.

use std::sync::Arc;
use std::time::Instant;

use yk_core::event::EventBus;
use yk_core::ports::{PluginHost, SearchIndex};
use yk_scrape::ScrapeEngine;
use yk_store::Store;

use crate::badges::BadgeService;
use crate::storage::Storage;
use crate::config::Config;

pub struct Services {
    pub store: Store,
    pub search: Arc<dyn SearchIndex>,
    /// Identifier detection and metadata lookup for quick-add.
    pub scrape: Arc<ScrapeEngine>,
    /// Attachment bytes on disk.
    pub storage: Arc<Storage>,
    pub events: EventBus,
    pub default_library: i64,
}

pub struct AppState {
    pub services: Arc<Services>,
    pub plugins: Arc<dyn PluginHost>,
    /// Sits beside the registry rather than inside it: badges are a *use* of
    /// plugins, not part of running them.
    pub badges: BadgeService,
    /// The configuration as it stands, which the workbench can change.
    pub config: parking_lot::RwLock<Config>,
    /// `None` when no model is configured, which is the default.
    ///
    /// Behind a lock because pointing the assistant at a different model is
    /// something a user does from the settings tab, and making them restart a
    /// server they did not start by hand would be a strange thing to ask.
    pub agent: parking_lot::RwLock<Option<Arc<yk_agent::Agent>>>,
    pub started: Instant,
    /// Agent turns in flight, one per conversation. A turn outlives the request
    /// that started it — see `runs`.
    pub runs: crate::runs::Runs,
    /// Word-processor sessions, one per open document.
    pub sessions: crate::integration::Sessions,
    /// Long jobs that outlive the request that started them.
    pub tasks: crate::tasks::Tasks,
    /// The sidebar's saved searches, with their counts, against the library
    /// version they were counted at.
    ///
    /// Here rather than in the store because a count comes from running the
    /// saved query through the *search engine* — 21ms each, and the sidebar
    /// asks for all of them on every navigation.
    pub smart_counts: Arc<yk_store::counts::Versioned<Vec<yk_core::model::SmartCollection>>>,
    /// The statistics panel, against the library version it was taken at.
    ///
    /// Every figure here is a label on a status bar, so one of them being a
    /// moment behind is invisible. What is not invisible is that two of them
    /// are exact counts of the whole library, recomputed inside the same
    /// first-paint burst that the item list is already counting in.
    pub stats: Arc<yk_store::counts::Versioned<serde_json::Value>>,
    /// Whether the browser-connector port was actually bound.
    ///
    /// Separate from `config.connector_port`, which only records that it was
    /// *asked for*. The bind is allowed to fail — a running Zotero already owns
    /// that port — and the server carries on, so the request and the outcome
    /// are genuinely different facts. Reporting the request would tell somebody
    /// browser saving is on at the exact moment it is not.
    pub connector_bound: Arc<std::sync::atomic::AtomicBool>,
}

/// What browser saving is doing, for the settings page to explain.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ConnectorStatus {
    /// Not asked for. The default, and deliberately so: the port belongs to
    /// Zotero, and taking it would break a Zotero the user is still migrating
    /// from.
    Off,
    /// Listening, so the browser extension can save to this library.
    Listening { port: u16 },
    /// Asked for and refused — almost always because Zotero has it.
    Unavailable { port: u16 },
}

impl AppState {
    pub fn store(&self) -> &Store {
        &self.services.store
    }
    pub fn search(&self) -> &Arc<dyn SearchIndex> {
        &self.services.search
    }
    pub fn events(&self) -> &EventBus {
        &self.services.events
    }
    pub fn scrape(&self) -> &Arc<ScrapeEngine> {
        &self.services.scrape
    }
    pub fn storage(&self) -> &Arc<Storage> {
        &self.services.storage
    }
    pub fn sessions(&self) -> &crate::integration::Sessions {
        &self.sessions
    }
    pub fn tasks(&self) -> &crate::tasks::Tasks {
        &self.tasks
    }

    /// What browser saving is doing, as opposed to what was asked of it.
    pub fn connector_status(&self) -> ConnectorStatus {
        match self.config().connector_port {
            None => ConnectorStatus::Off,
            Some(port) if self.connector_bound.load(std::sync::atomic::Ordering::Relaxed) => {
                ConnectorStatus::Listening { port }
            }
            Some(port) => ConnectorStatus::Unavailable { port },
        }
    }
    /// The agent as it is right now.
    ///
    /// Returns an owned handle rather than a borrow: a turn outlives the lock,
    /// and holding a read guard across one would block every reconfiguration
    /// for as long as the model takes to answer.
    pub fn agent(&self) -> Option<Arc<yk_agent::Agent>> {
        self.agent.read().clone()
    }

    /// A snapshot of the configuration.
    pub fn config(&self) -> Config {
        self.config.read().clone()
    }

    /// Every tool that could exist, whether or not it is switched on.
    pub fn tool_catalogue(&self) -> Vec<String> {
        let config = self.config();
        let skills = std::sync::Arc::new(yk_agent::skills::Skills::load_dir(&config.skills_dir()));
        let workspace = crate::agent::Workspace::new(config.workspace_dir()).ok();
        crate::agent::tool_catalogue(
            self.store(),
            self.search(),
            self.scrape(),
            workspace.as_ref(),
            &skills,
        )
    }
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

pub type App = Arc<AppState>;
