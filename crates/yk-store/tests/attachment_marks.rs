//! A listed row says what it has attached.
//!
//! The marks are derived from the child attachments on every read rather than
//! stored, so the thing worth testing is that the derivation survives a real
//! round trip: created children, a page query, and the ordering rule.

use yk_core::model::{AttachmentKind, ItemDraft};
use yk_core::query::{ItemFilter, ItemQuery};
use yk_store::Store;

struct Root(std::path::PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "yk-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> std::path::PathBuf {
        self.0.join("test.db")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[tokio::test]
async fn a_listed_row_reports_what_is_attached_to_it() {
    let root = Root::new("attach-marks");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    let paper = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "With a PDF"))
        .await
        .unwrap();
    let bare = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "With nothing"))
        .await
        .unwrap();

    // Deliberately out of display order, and with two files of the same kind,
    // so both the ordering rule and the de-duplication are exercised.
    for (content_type, link_mode) in [
        ("image/png", "imported_file"),
        ("text/html", "imported_url"),
        ("application/pdf", "imported_url"),
        ("image/jpeg", "imported_file"),
    ] {
        store
            .items
            .create(
                lib,
                ItemDraft {
                    parent_key: Some(paper.key.clone()),
                    ..ItemDraft::new("attachment")
                        .with_field("contentType", content_type)
                        .with_field("linkMode", link_mode)
                },
            )
            .await
            .unwrap();
    }

    let page = store
        .items
        .list(&ItemQuery {
            filter: ItemFilter { library_id: lib, top_level_only: true, ..Default::default() },
            ..Default::default()
        })
        .await
        .unwrap();

    let with_pdf = page.items.iter().find(|i| i.key == paper.key).expect("the paper");
    assert_eq!(
        with_pdf.attachments,
        vec![AttachmentKind::Pdf, AttachmentKind::Snapshot, AttachmentKind::File],
        "most telling kind first, each kind once"
    );

    let empty = page.items.iter().find(|i| i.key == bare.key).expect("the bare item");
    assert!(empty.attachments.is_empty(), "nothing attached, nothing claimed");
}

#[tokio::test]
async fn a_deleted_attachment_stops_being_reported() {
    let root = Root::new("attach-trash");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    let paper = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Trashed file"))
        .await
        .unwrap();
    let file = store
        .items
        .create(
            lib,
            ItemDraft {
                parent_key: Some(paper.key.clone()),
                ..ItemDraft::new("attachment")
                    .with_field("contentType", "application/pdf")
                    .with_field("linkMode", "imported_file")
            },
        )
        .await
        .unwrap();

    store.items.set_trashed(lib, &[file.key], true).await.unwrap();

    let page = store
        .items
        .list(&ItemQuery {
            filter: ItemFilter { library_id: lib, top_level_only: true, ..Default::default() },
            ..Default::default()
        })
        .await
        .unwrap();
    let row = page.items.iter().find(|i| i.key == paper.key).expect("the paper");
    assert!(row.attachments.is_empty(), "a trashed attachment is not an attachment");
}

#[test]
fn a_link_is_told_from_a_downloaded_page_by_its_link_mode() {
    // Both are `text/html`; only `link_mode` says whether anything was saved.
    assert_eq!(
        AttachmentKind::classify(Some("text/html"), Some("imported_url")),
        AttachmentKind::Snapshot
    );
    assert_eq!(
        AttachmentKind::classify(Some("text/html"), Some("linked_url")),
        AttachmentKind::Link
    );
    // A link with no content type at all is still a link.
    assert_eq!(AttachmentKind::classify(None, Some("linked_url")), AttachmentKind::Link);
    assert_eq!(AttachmentKind::classify(None, None), AttachmentKind::File);
}

/// The file browser needs the parent beside each file, and it now comes from
/// the same join that finds the file. A second pass over the parents used to
/// cost a third of a rename preview, so the risk in removing it is that the
/// parent quietly turns up empty — which only a test that looks at it catches.
#[tokio::test]
async fn a_listed_file_arrives_with_its_parent() {
    let root = Root::new("attach-parent");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    let paper = store
        .items
        .create(
            lib,
            ItemDraft::new("journalArticle")
                .with_field("title", "Parent paper")
                .with_field("date", "2019"),
        )
        .await
        .unwrap();
    store
        .items
        .create(
            lib,
            ItemDraft {
                parent_key: Some(paper.key.clone()),
                ..ItemDraft::new("attachment")
                    .with_field("filename", "paper.pdf")
                    .with_field("contentType", "application/pdf")
            },
        )
        .await
        .unwrap();
    // A loose file, to prove the `LEFT` in the join is still doing its job.
    store
        .items
        .create(lib, ItemDraft::new("attachment").with_field("filename", "orphan.pdf"))
        .await
        .unwrap();

    let page = store.items.attachments(lib, 100, 0).await.unwrap();
    assert_eq!(page.total, 2);

    let (_, parent) = page
        .items
        .iter()
        .find(|(a, _)| a.field("filename") == Some("paper.pdf"))
        .expect("the attached file");
    let parent = parent.as_ref().expect("its parent");
    assert_eq!(parent.key, paper.key);
    // Renaming reads the parent's title and year, so those fields have to have
    // survived the trip, not just the key.
    assert_eq!(parent.title(), "Parent paper");
    assert_eq!(parent.field("date"), Some("2019"));

    let (_, none) = page
        .items
        .iter()
        .find(|(a, _)| a.field("filename") == Some("orphan.pdf"))
        .expect("the loose file");
    assert!(none.is_none(), "a file with no parent must not invent one");
}

/// The sortable rank must agree with what the row reports, always.
///
/// It is a stored column kept up to date by trigger rather than by the write
/// paths, because there are five of those and the one that gets forgotten is
/// invisible: the column goes stale and a column sorts wrongly in a library
/// nobody is looking at yet. So the test is a sequence of ordinary edits, with
/// the rank checked after each.
#[tokio::test]
async fn the_attachment_rank_follows_every_kind_of_edit() {
    let root = Root::new("attach-rank");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    let rank = |key: yk_core::Key| {
        let store = &store;
        async move {
            let page = store
                .items
                .list(&ItemQuery {
                    filter: ItemFilter {
                        library_id: lib,
                        top_level_only: true,
                        ..Default::default()
                    },
                    sort: yk_core::query::SortField::Attachment,
                    ..Default::default()
                })
                .await
                .unwrap();
            // The rank is not on the wire; what it must agree with is the marks.
            page.items
                .iter()
                .find(|i| i.key == key)
                .map(|i| i.attachments.clone())
                .unwrap_or_default()
        }
    };

    let paper = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Ranked"))
        .await
        .unwrap();
    assert!(rank(paper.key.clone()).await.is_empty(), "nothing attached yet");

    let file = store
        .items
        .create(
            lib,
            ItemDraft {
                parent_key: Some(paper.key.clone()),
                ..ItemDraft::new("attachment")
                    .with_field("filename", "page.html")
                    .with_field("contentType", "text/html")
            },
        )
        .await
        .unwrap();
    assert_eq!(rank(paper.key.clone()).await, vec![AttachmentKind::Snapshot]);

    // Patched into a PDF: the update path, not the insert path.
    let patch = serde_json::from_value(serde_json::json!({
        "fields": { "contentType": "application/pdf" }
    }))
    .unwrap();
    store.items.update(lib, &file.key, patch, None).await.unwrap();
    assert_eq!(rank(paper.key.clone()).await, vec![AttachmentKind::Pdf]);

    // Trashed: soft-deleted, which is an update rather than a delete.
    store.items.set_trashed(lib, std::slice::from_ref(&file.key), true).await.unwrap();
    assert!(rank(paper.key.clone()).await.is_empty(), "a trashed file is not attached");

    // And back again.
    store.items.set_trashed(lib, std::slice::from_ref(&file.key), false).await.unwrap();
    assert_eq!(rank(paper.key.clone()).await, vec![AttachmentKind::Pdf]);
}

/// Sorting puts the papers with files at the top.
#[tokio::test]
async fn sorting_by_attachment_ranks_pdfs_above_nothing() {
    let root = Root::new("attach-sort");
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;

    let bare = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "Nothing"))
        .await
        .unwrap();
    let linked = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "A link"))
        .await
        .unwrap();
    let withpdf = store
        .items
        .create(lib, ItemDraft::new("journalArticle").with_field("title", "A PDF"))
        .await
        .unwrap();

    for (parent, content_type, link_mode) in [
        (&linked.key, "text/html", "linked_url"),
        (&withpdf.key, "application/pdf", "imported_file"),
    ] {
        store
            .items
            .create(
                lib,
                ItemDraft {
                    parent_key: Some(parent.clone()),
                    ..ItemDraft::new("attachment")
                        .with_field("filename", "f")
                        .with_field("contentType", content_type)
                        .with_field("linkMode", link_mode)
                },
            )
            .await
            .unwrap();
    }

    let page = store
        .items
        .list(&ItemQuery {
            filter: ItemFilter { library_id: lib, top_level_only: true, ..Default::default() },
            sort: yk_core::query::SortField::Attachment,
            direction: yk_core::query::Direction::Desc,
            ..Default::default()
        })
        .await
        .unwrap();

    let order: Vec<&str> = page.items.iter().map(|i| i.title()).collect();
    assert_eq!(order, ["A PDF", "A link", "Nothing"], "best first, empty last");

    // And the other way round, since a column header toggles.
    let page = store
        .items
        .list(&ItemQuery {
            filter: ItemFilter { library_id: lib, top_level_only: true, ..Default::default() },
            sort: yk_core::query::SortField::Attachment,
            direction: yk_core::query::Direction::Asc,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items[0].title(), "Nothing");
    let _ = bare;
}
