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
    pub config: Config,
    /// `None` when no model is configured, which is the default.
    pub agent: Option<Arc<yk_agent::Agent>>,
    pub started: Instant,
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
    pub fn agent(&self) -> Option<&yk_agent::Agent> {
        self.agent.as_deref()
    }
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

pub type App = Arc<AppState>;
