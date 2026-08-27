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
    /// The reference-harvesting run, if one is going. One at a time: they all
    /// talk to the same service, and two would only get the client throttled.
    pub harvest: parking_lot::Mutex<crate::routes::Harvest>,
    /// Agent turns in flight, one per conversation. A turn outlives the request
    /// that started it — see `runs`.
    pub runs: crate::runs::Runs,
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
