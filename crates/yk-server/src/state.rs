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
use yk_store::Store;

use crate::config::Config;

pub struct Services {
    pub store: Store,
    pub search: Arc<dyn SearchIndex>,
    pub events: EventBus,
    pub default_library: i64,
}

pub struct AppState {
    pub services: Arc<Services>,
    pub plugins: Arc<dyn PluginHost>,
    pub config: Config,
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
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

pub type App = Arc<AppState>;
