use serde::Serialize;
use tokio::sync::broadcast;

use crate::Key;

/// Facts about things that already happened. Published after the storage
/// transaction commits, consumed by the WebSocket fan-out, the search indexer
/// and the plugin hook bus.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DomainEvent {
    #[serde(rename_all = "camelCase")]
    ItemsChanged { library_id: i64, keys: Vec<Key>, version: i64 },
    #[serde(rename_all = "camelCase")]
    ItemsTrashed { library_id: i64, keys: Vec<Key>, version: i64 },
    #[serde(rename_all = "camelCase")]
    ItemsDeleted { library_id: i64, keys: Vec<Key>, version: i64 },
    #[serde(rename_all = "camelCase")]
    CollectionsChanged { library_id: i64, version: i64 },
    #[serde(rename_all = "camelCase")]
    TagsChanged { library_id: i64 },
    #[serde(rename_all = "camelCase")]
    IndexProgress { done: i64, total: i64 },
    /// An agent turn's state changed. Carries the whole state rather than a
    /// delta: it is small, and a client that missed one delta would otherwise
    /// be permanently out of step with no way to notice.
    #[serde(rename_all = "camelCase")]
    AgentProgress { library_id: i64, conversation: String, state: serde_json::Value },
    #[serde(rename_all = "camelCase")]
    PluginsChanged,
    #[serde(rename_all = "camelCase")]
    Log { level: String, message: String },
}

/// Broadcast bus. Slow subscribers are dropped rather than blocking writers.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<DomainEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: DomainEvent) {
        // An error only means "no subscribers", which is not a failure.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.tx.subscribe()
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}
