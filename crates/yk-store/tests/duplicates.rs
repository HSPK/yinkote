//! Finding duplicates and folding them together.
//!
//! Merging is the one operation in a reference manager that a user cannot put
//! right by hand: the losing record is gone, and with it whatever was only on
//! that copy — the PDF, the notes, the collection it was filed in. So the
//! behaviour worth pinning down is not "does it merge" but "what does it take
//! care not to lose".

use yk_core::model::{Creator, ItemDraft, ItemTag};
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

fn paper(title: &str, surname: &str, year: &str) -> ItemDraft {
    ItemDraft::new("journalArticle")
        .with_field("title", title)
        .with_field("date", year)
        .with_creator(Creator {
            creator_type: "author".into(),
            last_name: Some(surname.into()),
            first_name: Some("A".into()),
            name: None,
        })
}

async fn store(name: &str) -> (Root, Store, i64) {
    let root = Root::new(name);
    let store = Store::open(Some(&root.db())).unwrap();
    let lib = store.default_library;
    (root, store, lib)
}

#[tokio::test]
async fn two_records_of_one_paper_are_found() {
    let (_root, store, lib) = store("dup-scan").await;

    store.items.create(lib, paper("Attention Is All You Need", "Vaswani", "2017")).await.unwrap();
    store.items.create(lib, paper("attention is all you need", "Vaswani", "2017")).await.unwrap();
    store.items.create(lib, paper("Something Else", "Zhang", "2020")).await.unwrap();

    let groups = store.items.duplicate_groups(lib, 50).await.unwrap();
    assert_eq!(groups.len(), 1, "one paper filed twice, and one filed once");
    assert_eq!(groups[0].len(), 2);
}

#[tokio::test]
async fn a_shared_doi_is_a_duplicate_however_the_title_was_typed() {
    let (_root, store, lib) = store("dup-doi").await;

    store
        .items
        .create(lib, paper("Deep Residual Learning", "He", "2016").with_field("DOI", "10.1109/CVPR.2016.90"))
        .await
        .unwrap();
    store
        .items
        .create(
            lib,
            paper("Deep residual learning for image recognition", "He, Kaiming", "2016")
                .with_field("DOI", "10.1109/cvpr.2016.90"),
        )
        .await
        .unwrap();

    let groups = store.items.duplicate_groups(lib, 50).await.unwrap();
    assert_eq!(groups.len(), 1, "the identifier settles it, not the typing");
}

#[tokio::test]
async fn items_with_nothing_to_identify_them_are_not_all_duplicates() {
    let (_root, store, lib) = store("dup-empty").await;

    // Three blank records fingerprint identically. Reporting them as one group
    // of duplicates is technically true and completely useless: there is
    // nothing to compare and nothing to keep.
    for _ in 0..3 {
        store.items.create(lib, ItemDraft::new("document")).await.unwrap();
    }

    let groups = store.items.duplicate_groups(lib, 50).await.unwrap();
    assert!(groups.is_empty());
}

#[tokio::test]
async fn an_attachment_is_not_a_duplicate_of_another_attachment() {
    let (_root, store, lib) = store("dup-children").await;

    let a = store.items.create(lib, paper("Host One", "Zhang", "2020")).await.unwrap();
    let b = store.items.create(lib, paper("Host Two", "Li", "2021")).await.unwrap();
    for parent in [&a.key, &b.key] {
        store
            .items
            .create(
                lib,
                ItemDraft {
                    parent_key: Some(parent.clone()),
                    ..ItemDraft::new("attachment").with_field("filename", "paper.pdf")
                },
            )
            .await
            .unwrap();
    }

    // Both attachments have no title, author or year of their own.
    let groups = store.items.duplicate_groups(lib, 50).await.unwrap();
    assert!(groups.is_empty(), "files are compared through their parents, not with each other");
}

#[tokio::test]
async fn a_copy_with_an_identifier_matches_one_without() {
    let (_root, store, lib) = store("dup-mixed").await;

    // The commonest duplicate of all: one copy imported from the publisher,
    // one typed by hand. Their fingerprints differ — a fingerprint prefers an
    // identifier — so only comparing what is written on the paper finds them.
    store.items.create(lib, paper("A Shared Paper", "Kim", "2019")).await.unwrap();
    store
        .items
        .create(lib, paper("A Shared Paper", "Kim", "2019").with_field("DOI", "10.5555/dup"))
        .await
        .unwrap();

    let groups = store.items.duplicate_groups(lib, 50).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 2);
}

#[tokio::test]
async fn records_linked_by_different_matches_form_one_group() {
    let (_root, store, lib) = store("dup-chain").await;

    // A and B share a DOI; B and C share a title. All three are the same paper,
    // and showing B in two groups would invite merging it into two masters.
    store
        .items
        .create(lib, paper("Deep Residual Learning", "He", "2016").with_field("DOI", "10.1/x"))
        .await
        .unwrap();
    store
        .items
        .create(lib, paper("Residual Nets", "He", "2016").with_field("DOI", "10.1/x"))
        .await
        .unwrap();
    store.items.create(lib, paper("Residual Nets", "He", "2016")).await.unwrap();

    let groups = store.items.duplicate_groups(lib, 50).await.unwrap();
    assert_eq!(groups.len(), 1, "one paper, one group");
    assert_eq!(groups[0].len(), 3);
}

#[tokio::test]
async fn merging_keeps_what_was_only_on_the_copy_being_discarded() {
    let (_root, store, lib) = store("merge-keep").await;

    let collection = store
        .collections
        .create(lib, yk_core::model::CollectionDraft { name: "Reading".into(), ..Default::default() })
        .await
        .unwrap();

    let master = store.items.create(lib, paper("A Paper", "Zhang", "2020")).await.unwrap();

    // The copy about to be discarded is the one with the PDF, the DOI, the
    // abstract, a tag and a collection — which is the usual way round, because
    // it is the copy that was imported from somewhere that knew more.
    let other = store
        .items
        .create(
            lib,
            paper("A Paper", "Zhang", "2020")
                .with_field("DOI", "10.1/xyz")
                .with_field("abstractNote", "The abstract."),
        )
        .await
        .unwrap();
    let file = store
        .items
        .create(
            lib,
            ItemDraft {
                parent_key: Some(other.key.clone()),
                ..ItemDraft::new("attachment").with_field("filename", "paper.pdf")
            },
        )
        .await
        .unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "tags": [ItemTag::manual("transformers")],
        "collections": [collection.key.to_string()],
    }))
    .unwrap();
    store.items.update(lib, &other.key, patch, None).await.unwrap();

    let merged = store.items.merge(lib, &master.key, std::slice::from_ref(&other.key)).await.unwrap();

    assert_eq!(merged.field("DOI"), Some("10.1/xyz"), "an identifier the master lacked");
    assert_eq!(merged.field("abstractNote"), Some("The abstract."));
    assert_eq!(
        merged.tags.iter().map(|t| t.tag.as_str()).collect::<Vec<_>>(),
        vec!["transformers"],
        "and its tag — named, because a count cannot tell which tag survived",
    );
    assert_eq!(merged.collections, vec![collection.key], "and where it was filed");

    let child = store.items.get(lib, &file.key).await.unwrap();
    assert_eq!(child.parent_key.as_ref(), Some(&master.key), "the PDF moved across");

    // Recoverable. Everything else about a merge is a matter of taste; this is
    // the part that has to be true.
    let gone = store.items.get(lib, &other.key).await.unwrap();
    assert!(gone.deleted, "the loser is in the trash, not destroyed");

    let live = store
        .items
        .list(&ItemQuery {
            filter: ItemFilter { library_id: lib, top_level_only: true, ..Default::default() },
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(live.total, 1, "and the library shows one paper now");
}

#[tokio::test]
async fn merging_never_overwrites_the_record_that_was_chosen() {
    let (_root, store, lib) = store("merge-master-wins").await;

    let master = store
        .items
        .create(lib, paper("The Better Record", "Zhang", "2020").with_field("volume", "12"))
        .await
        .unwrap();
    let other = store
        .items
        .create(lib, paper("The Better Record", "Zhang", "2020").with_field("volume", "99"))
        .await
        .unwrap();

    let merged = store.items.merge(lib, &master.key, &[other.key]).await.unwrap();

    // The user picked this record. A merge may fill in a blank; it may not
    // decide that the other copy knew better.
    assert_eq!(merged.field("volume"), Some("12"));
    assert_eq!(merged.title(), "The Better Record");
}

#[tokio::test]
async fn merging_is_one_change_at_one_version() {
    let (_root, store, lib) = store("merge-version").await;

    let master = store.items.create(lib, paper("A Paper", "Zhang", "2020")).await.unwrap();
    let one = store.items.create(lib, paper("A Paper", "Zhang", "2020")).await.unwrap();
    let two = store.items.create(lib, paper("A Paper", "Zhang", "2020")).await.unwrap();
    let before = store.libraries.version(lib).await.unwrap();

    let merged = store.items.merge(lib, &master.key, &[one.key, two.key]).await.unwrap();

    assert_eq!(store.libraries.version(lib).await.unwrap(), before + 1);
    assert_eq!(merged.version, before + 1, "the master carries that version");
    assert!(store.items.duplicate_groups(lib, 50).await.unwrap().is_empty(), "and they are gone");
}

#[tokio::test]
async fn merging_a_record_into_itself_does_nothing() {
    let (_root, store, lib) = store("merge-self").await;
    let master = store.items.create(lib, paper("A Paper", "Zhang", "2020")).await.unwrap();
    let before = store.libraries.version(lib).await.unwrap();

    // The selection included the master, as a select-all naturally would.
    let merged = store.items.merge(lib, &master.key, std::slice::from_ref(&master.key)).await.unwrap();

    assert!(!merged.deleted, "it must not trash the very record it was asked to keep");
    assert_eq!(store.libraries.version(lib).await.unwrap(), before, "and nothing was written");
}

/// A blank field on the master is a gap, not a value worth keeping.
///
/// `merging_never_overwrites_the_record_that_was_chosen` above proves a real
/// value survives; this is the boundary of that rule. A record typed by hand
/// often has a field that was tabbed through and left as spaces, and treating
/// that as something the user chose would keep the blank and drop the DOI the
/// other copy actually had.
#[tokio::test]
async fn an_empty_field_on_the_master_counts_as_missing() {
    let (_root, store, lib) = store("merge-blank-is-a-gap").await;

    let master = store
        .items
        .create(lib, paper("Blanks", "Li", "2021").with_field("DOI", "   "))
        .await
        .unwrap();
    let other = store
        .items
        .create(lib, paper("Blanks", "Li", "2021").with_field("DOI", "10.3/real"))
        .await
        .unwrap();

    let merged = store.items.merge(lib, &master.key, &[other.key]).await.unwrap();
    assert_eq!(merged.field("DOI"), Some("10.3/real"), "whitespace was treated as a real value");

    // Re-read rather than trusting the reply; those have disagreed before.
    let stored = store.items.get(lib, &master.key).await.unwrap();
    assert_eq!(stored.field("DOI"), Some("10.3/real"));
}
