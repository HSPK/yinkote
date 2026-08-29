//! Reading a `.yinkote` archive back into a library.
//!
//! The other half of [`super::export`], and the half that makes it a door
//! rather than a hole: a file nothing can read is not portability.
//!
//! It **merges** rather than replaces. On a fresh machine the target library is
//! empty and merging is restoring; on a machine that already has a library it
//! is the only safe reading of "import" — replacing would destroy work that
//! nobody asked about. Anything already present, by key, is left exactly as it
//! is and counted as skipped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use yk_core::model::{CollectionDraft, Item, ItemDraft};
use yk_core::query::{ItemFilter, ItemQuery};
use yk_core::{Error, Key, Result};

use crate::state::App;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Restored {
    pub items: u64,
    /// Already here under the same key, and left alone.
    pub skipped: u64,
    pub files: u64,
    /// Shelves rebuilt, with their keys and their nesting.
    pub collections: u64,
    /// Records the archive held that could not be written. Counted rather than
    /// fatal: an import that stops half way through is worse than one that
    /// finishes and says what it could not do.
    pub failed: u64,
    /// Why, for the first few. A count on its own tells somebody their backup
    /// is incomplete and nothing about what to do next.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

/// Read an archive into the library.
/// Returns what was restored, and whether it stopped early because it was
/// asked to. The two are separate because a job that stopped still did work,
/// and because asking a job to stop is not the same as it stopping.
pub async fn run(
    app: &App,
    archive: &Path,
    task: &Arc<crate::tasks::Task>,
) -> Result<(Restored, bool)> {
    if !archive.exists() {
        return Err(Error::invalid(format!("{} does not exist", archive.display())));
    }

    // Unpacked into a directory of its own, and removed afterwards. Reading
    // members straight out of the zip would mean holding the whole database in
    // memory, and a library's database is measured in hundreds of megabytes.
    let staging = app.config().data_dir().join(format!(".import-{}", std::process::id()));
    std::fs::remove_dir_all(&staging).ok();
    let unpack = staging.clone();
    let source = archive.to_path_buf();
    let files_in_archive =
        tokio::task::spawn_blocking(move || unpack_archive(&source, &unpack))
            .await
            .map_err(|e| Error::internal(format!("unpacking did not finish: {e}")))??;

    let result = merge(app, &staging, files_in_archive, task).await;
    std::fs::remove_dir_all(&staging).ok();
    result
}

/// Extract the archive, returning the attachment paths it held.
///
/// Every member is checked to stay inside the staging directory. A zip is a
/// file somebody else made, and `../../.ssh/authorized_keys` is a valid name
/// for a member — this is the oldest trap in the format and it is worth being
/// explicit about refusing it.
fn unpack_archive(archive: &Path, into: &Path) -> Result<Vec<(String, PathBuf)>> {
    let file = std::fs::File::open(archive)
        .map_err(|e| Error::invalid(format!("could not open the archive: {e}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| Error::invalid(format!("not a readable archive: {e}")))?;

    std::fs::create_dir_all(into)
        .map_err(|e| Error::internal(format!("could not stage the import: {e}")))?;

    let mut attachments = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| Error::invalid(format!("archive: {e}")))?;
        let Some(name) = entry.enclosed_name() else {
            // `enclosed_name` is `None` for anything that would escape.
            continue;
        };
        let name = name.to_path_buf();
        let target = into.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).ok();
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::internal(format!("could not stage {name:?}: {e}")))?;
        }
        let mut out = std::fs::File::create(&target)
            .map_err(|e| Error::internal(format!("could not write {name:?}: {e}")))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| Error::internal(format!("could not unpack {name:?}: {e}")))?;

        // `library/<key>/<filename>` is where an attachment's bytes live.
        let parts: Vec<String> =
            name.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        if parts.len() == 3 && parts[0] == "library" {
            attachments.push((parts[1].clone(), target));
        }
    }
    Ok(attachments)
}

/// Copy what the staged archive holds into the running library.
async fn merge(
    app: &App,
    staging: &Path,
    files: Vec<(String, PathBuf)>,
    task: &Arc<crate::tasks::Task>,
) -> Result<(Restored, bool)> {
    let db = staging.join("db.sqlite");
    if !db.exists() {
        return Err(Error::invalid("the archive holds no database"));
    }

    // Opened as a library in its own right: the archive has this program's
    // schema, so reading it with this program's repositories means the import
    // cannot drift from the model. Ad-hoc SQL over somebody else's snapshot is
    // how an importer ends up understanding an older schema than it runs.
    let source = yk_store::Store::open(Some(&db))
        .map_err(|e| Error::invalid(format!("the archive's database will not open: {e}")))?;
    let source_lib = source.default_library;
    let target = app.store().default_library;

    // Shelves before the papers on them.
    //
    // Without this, restoring into a *fresh* library dropped every collection
    // and then failed every item that belonged to one — the draft named a
    // collection that did not exist. Nothing said so: the count of failures
    // was reported with the reasons thrown away.
    //
    // The bug hid because the obvious test restores into the library the
    // archive came from, where the shelves are still there. The case that
    // matters is the other one: a new machine, an empty library, and a backup.
    let collections = restore_collections(app, &source, source_lib, target).await?;
    let mut out = Restored { collections, ..Default::default() };

    let mut offset = 0u32;
    // Parents before children: a child whose parent has not arrived cannot be
    // filed under it, and the archive lists items in id order, which is the
    // order they were created in — parents first.
    loop {
        let page = source
            .items
            .list(&ItemQuery {
                filter: ItemFilter {
                    library_id: source_lib,
                    trash: yk_core::query::TrashScope::Include,
                    ..Default::default()
                },
                sort: yk_core::query::SortField::DateAdded,
                direction: yk_core::query::Direction::Asc,
                limit: 500,
                offset,
            })
            .await?;
        if page.items.is_empty() {
            break;
        }
        offset += page.items.len() as u32;
        // Stopping between pages rather than mid-page: an import that halts
        // half way through a batch leaves the library in a state nobody chose.
        // What has already been written stays — it is merged, so it is exactly
        // what a second attempt would skip.
        if task.cancelled() {
            return Ok((out, true));
        }
        task.progress("task.restoringItems", u64::from(offset), page.total.max(0) as u64);

        // A page at a time, not an item at a time. Asking `get` per item is a
        // hundred thousand round trips on a real library: it takes minutes, and
        // it holds a pooled connection almost continuously, so an ordinary
        // write elsewhere in the program waits out its busy timeout and fails
        // with "database is locked". The library is not locked; the pool is
        // busy, and the difference is invisible from the error.
        let keys: Vec<Key> = page.items.iter().map(|i| i.key.clone()).collect();
        let present: std::collections::HashSet<String> = app
            .store()
            .items
            .get_many(target, &keys)
            .await?
            .into_iter()
            .map(|i| i.key.to_string())
            .collect();

        let mut drafts = Vec::with_capacity(page.items.len());
        for item in &page.items {
            if present.contains(&item.key.to_string()) {
                // Already here. Leaving it alone is the whole point of merging.
                out.skipped += 1;
            } else {
                drafts.push(draft_of(item));
            }
        }
        if !drafts.is_empty() {
            for result in app.store().items.create_many(target, drafts).await? {
                match result {
                    Ok(_) => out.items += 1,
                    // Counted *and* said. A bare "failed: 1" on a restore is
                    // the least useful thing a backup can tell somebody.
                    Err(e) => {
                        tracing::warn!(error = %e, "an item could not be restored");
                        if out.reasons.len() < 10 {
                            out.reasons.push(e.to_string());
                        }
                        out.failed += 1
                    }
                }
            }
        }
        // Trash is part of what was backed up. An item the user threw away
        // coming back alive is the restore quietly undoing a decision they
        // made — and on a real library it arrives as hundreds of papers they
        // had already dealt with.
        let binned: Vec<Key> =
            page.items.iter().filter(|i| i.deleted).map(|i| i.key.clone()).collect();
        if !binned.is_empty() {
            app.store().items.set_trashed(target, &binned, true).await?;
        }
        if page.items.len() < 500 {
            break;
        }
    }

    let total_files = files.len() as u64;
    for (i, (key, path)) in files.into_iter().enumerate() {
        if task.cancelled() {
            return Ok((out, true));
        }
        if i % 20 == 0 {
            task.progress("task.restoringFiles", i as u64, total_files);
        }
        let Ok(key) = Key::parse(&key) else {
            out.failed += 1;
            continue;
        };
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            out.failed += 1;
            continue;
        };
        match std::fs::read(&path) {
            Ok(bytes) => match app.storage().put(&key, &name, &bytes).await {
                Ok(_) => out.files += 1,
                Err(_) => out.failed += 1,
            },
            Err(_) => out.failed += 1,
        }
    }

    Ok((out, false))
}

/// Rebuild the archive's shelves in the target library, parents first.
///
/// Keys travel, as they do for items: a collection's key is what every item's
/// membership names, so a renumbered shelf is an empty one.
///
/// Ordering is the whole difficulty. A child cannot be created before its
/// parent exists, and the archive lists collections in no particular order, so
/// this places whatever it can on each pass and stops when a pass places
/// nothing. Anything left is a cycle or an orphan — neither should exist, and
/// both are filed at the top level rather than dropped, because a shelf in the
/// wrong place is recoverable and a missing one is not.
async fn restore_collections(
    app: &App,
    source: &yk_store::Store,
    source_lib: i64,
    target: i64,
) -> Result<u64> {
    let archived = source.collections.list(source_lib).await?;
    let existing: std::collections::HashSet<String> = app
        .store()
        .collections
        .list(target)
        .await?
        .into_iter()
        .map(|c| c.key.to_string())
        .collect();

    let mut placed: std::collections::HashSet<String> = existing.clone();
    let mut pending: Vec<&yk_core::model::Collection> =
        archived.iter().filter(|c| !existing.contains(&c.key.to_string())).collect();
    let mut restored = 0u64;

    while !pending.is_empty() {
        let mut stuck = true;
        let mut next = Vec::with_capacity(pending.len());
        for collection in pending {
            let ready = match &collection.parent_key {
                None => true,
                Some(parent) => placed.contains(&parent.to_string()),
            };
            if !ready {
                next.push(collection);
                continue;
            }
            let draft = CollectionDraft {
                name: collection.name.clone(),
                parent_key: collection.parent_key.clone(),
                sort_index: Some(collection.sort_index),
                color: collection.color.clone(),
                icon: collection.icon.clone(),
                key: Some(collection.key.clone()),
            };
            match app.store().collections.create(target, draft).await {
                Ok(_) => {
                    placed.insert(collection.key.to_string());
                    restored += 1;
                    stuck = false;
                }
                Err(e) => {
                    tracing::warn!(error = %e, key = %collection.key, "a shelf could not be restored");
                }
            }
        }
        if stuck {
            // Nothing moved, so the rest are waiting on a parent that will
            // never arrive. Give them one at the top level.
            for collection in next {
                let draft = CollectionDraft {
                    name: collection.name.clone(),
                    parent_key: None,
                    sort_index: Some(collection.sort_index),
                    color: collection.color.clone(),
                    icon: collection.icon.clone(),
                    key: Some(collection.key.clone()),
                };
                if app.store().collections.create(target, draft).await.is_ok() {
                    restored += 1;
                }
            }
            break;
        }
        pending = next;
    }

    // A tag's colour is not on the tag rows the items carry; it is set apart
    // and was simply never copied.
    for tag in source.tags.list(source_lib, None, u32::MAX).await? {
        if let Some(colour) = tag.color.as_deref() {
            app.store().tags.set_color(target, &tag.name, Some(colour)).await.ok();
        }
    }

    Ok(restored)
}

/// Turn an archived item back into something creatable, keeping its identity.
///
/// The key travels: it is what parents, collections and citations refer to, and
/// a library whose items were renumbered on the way in has lost every link it
/// had. The same argument applies to when the item was added — restoring a
/// backup used to stamp the whole library with the moment of the restore,
/// which resorts every "date added" column and empties "recently added".
fn draft_of(item: &Item) -> ItemDraft {
    ItemDraft {
        item_type: item.item_type.clone(),
        fields: item.fields.clone(),
        creators: item.creators.clone(),
        tags: item.tags.clone(),
        collections: item.collections.clone(),
        parent_key: item.parent_key.clone(),
        key: Some(item.key.clone()),
        date_added: Some(item.date_added),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_item_keeps_the_day_it_was_added() {
        // Restoring used to stamp the whole library with the moment of the
        // restore. Nothing errors, nothing is missing, and every "date added"
        // column is quietly wrong — including "recently added", which becomes
        // the entire library.
        let mut item = ItemDraft::new("journalArticle")
            .with_field("title", "Filed years ago")
            .into_item(Key::generate(), 1, 3);
        item.date_added = 1_600_000_000_000;

        let draft = draft_of(&item);
        assert_eq!(draft.date_added, Some(1_600_000_000_000), "the archive knows this");
        // And it survives being turned back into an item.
        let again = draft.into_item(item.key.clone(), 1, 4);
        assert_eq!(again.date_added, item.date_added);
        // Modified is not preserved: it was modified, just now, by being written.
        assert!(again.date_modified >= item.date_added);
    }

    #[test]
    fn a_member_that_would_escape_the_directory_is_refused() {
        // `../../.ssh/authorized_keys` is a legal name for a zip member, and
        // an importer that joins it to a path writes wherever it is told.
        let dir = std::env::temp_dir().join(format!("yk-esc-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();

        let archive = dir.join("evil.zip");
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("../escaped.txt", opts).unwrap();
            zip.write_all(b"gotcha").unwrap();
            zip.start_file("library/AAAA1111/ok.pdf", opts).unwrap();
            zip.write_all(b"fine").unwrap();
            zip.finish().unwrap();
        }

        let into = dir.join("staging");
        let files = unpack_archive(&archive, &into).unwrap();

        assert!(!dir.join("escaped.txt").exists(), "wrote outside the staging directory");
        assert_eq!(files.len(), 1, "the legitimate file still arrives");
        assert_eq!(files[0].0, "AAAA1111");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn something_that_is_not_an_archive_is_refused_rather_than_half_read() {
        let dir = std::env::temp_dir().join(format!("yk-nodb-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();

        let not_a_zip = dir.join("holiday.jpg");
        std::fs::write(&not_a_zip, b"\xff\xd8\xff\xe0 not a zip at all").unwrap();
        let err = unpack_archive(&not_a_zip, &dir.join("staging")).unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Invalid, "the caller's fault, not ours");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_item_keeps_its_identity_on_the_way_back_in() {
        // The key is what parents, collections and citations point at. A
        // library whose items were renumbered on import has lost every link it
        // had, and nothing about the items themselves would look wrong.
        let original = ItemDraft::new("journalArticle")
            .with_field("title", "Kept")
            .into_item(Key::generate(), 1, 7);
        let mut child = ItemDraft::new("attachment").into_item(Key::generate(), 1, 7);
        child.parent_key = Some(original.key.clone());

        let draft = draft_of(&original);
        assert_eq!(draft.key.as_ref(), Some(&original.key));
        assert_eq!(draft_of(&child).parent_key, Some(original.key));
        // The version is not carried: it belongs to the library the item is
        // arriving in, not the one it left.
        assert_eq!(draft.item_type, "journalArticle");
    }
}
