use yk_core::{
    event::{DomainEvent, EventBus},
    Key, Result,
};
use yk_store::Store;

use crate::state::App;

pub async fn delete(
    store: &Store,
    events: &EventBus,
    library_id: i64,
    keys: &[Key],
) -> Result<(u64, i64)> {
    let deleted = store.items.delete(library_id, keys).await?;
    let version = store.libraries.version(library_id).await?;
    events.publish(DomainEvent::ItemsDeleted {
        library_id,
        keys: keys.to_vec(),
        version,
    });
    Ok((deleted, version))
}

/// A failed removal remains queued; no file is touched before its item commits.
pub async fn cleanup(app: &App) -> Result<usize> {
    let pending = app.store().db().pending_file_cleanup(200).await?;
    if pending.is_empty() {
        return Ok(0);
    }
    let mut removed = Vec::new();
    for (lib, key) in pending {
        match app.storage().remove(&key).await {
            Ok(()) => removed.push((lib, key)),
            Err(error) => tracing::warn!(%key, %error, "file cleanup will be retried"),
        }
    }
    let keys: Vec<Key> = removed.iter().map(|(_, key)| key.clone()).collect();
    crate::routes::thumbnails::forget(app, &keys).await?;
    let count = removed.len();
    app.store().db().finish_file_cleanup(removed).await?;
    Ok(count)
}
