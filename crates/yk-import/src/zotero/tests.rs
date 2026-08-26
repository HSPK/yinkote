//! Tests against a synthetic Zotero library.
//!
//! Building the schema here rather than shipping a fixture keeps the test
//! honest about which columns are actually depended on: anything not created
//! below is something the importer must not require.

use super::*;

/// The parts of Zotero's schema this importer reads.
fn zotero_library() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zotero.sqlite");
    let db = Connection::open(&path).unwrap();

    db.execute_batch(
        "CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT);
         CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INTEGER, key TEXT,
                             dateAdded TEXT, dateModified TEXT);
         CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY);
         CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT);
         CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value TEXT);
         CREATE TABLE itemData (itemID INTEGER, fieldID INTEGER, valueID INTEGER);
         CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT,
                                fieldMode INTEGER);
         CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
         CREATE TABLE itemCreators (itemID INTEGER, creatorID INTEGER, creatorTypeID INTEGER,
                                    orderIndex INTEGER);
         CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE itemTags (itemID INTEGER, tagID INTEGER, type INTEGER);
         CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT,
                                   key TEXT, parentCollectionID INTEGER);
         CREATE TABLE collectionItems (collectionID INTEGER, itemID INTEGER);
         CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INTEGER,
                                       path TEXT, contentType TEXT);
         CREATE TABLE itemNotes (itemID INTEGER PRIMARY KEY, parentItemID INTEGER,
                                 note TEXT, title TEXT);

         INSERT INTO itemTypes VALUES (1, 'journalArticle'), (2, 'attachment'), (3, 'note');
         INSERT INTO fields VALUES (1, 'title'), (2, 'DOI'), (3, 'abstractNote');
         INSERT INTO creatorTypes VALUES (1, 'author');

         INSERT INTO items VALUES (10, 1, 'AAAA1111', '2020-01-01', '2020-01-02'),
                                  (11, 1, 'BBBB2222', '2020-01-03', '2020-01-04'),
                                  (12, 1, 'CCCC3333', '2020-01-05', '2020-01-06'),
                                  (20, 2, 'DDDD4444', '2020-01-01', '2020-01-02');
         INSERT INTO deletedItems VALUES (12);
         INSERT INTO itemAttachments VALUES (20, 10, 'storage:paper.pdf', 'application/pdf');
         INSERT INTO items VALUES (21, 2, 'GGGG7777', '2020-01-01', '2020-01-02');
         INSERT INTO itemAttachments VALUES (21, 10, '/elsewhere/linked.pdf', 'application/pdf');
         INSERT INTO items VALUES (22, 3, 'HHHH8888', '2020-01-01', '2020-01-02'),
                                  (23, 3, 'IIII9999', '2020-01-01', '2020-01-02');
         INSERT INTO itemNotes VALUES (22, 10, '<p>The key idea is attention.</p>', 'Note'),
                                      (23, NULL, '<p>A standalone thought.</p>', 'Loose');

         INSERT INTO itemDataValues VALUES (1, 'Attention Is All You Need'),
                                           (2, '10.1000/xyz'), (3, 'We propose the Transformer.'),
                                           (4, 'Another Paper');
         INSERT INTO itemData VALUES (10, 1, 1), (10, 2, 2), (10, 3, 3), (11, 1, 4);

         INSERT INTO creators VALUES (1, 'Ashish', 'Vaswani', 0), (2, NULL, 'OpenAI', 1);
         INSERT INTO itemCreators VALUES (10, 2, 1, 1), (10, 1, 1, 0);

         INSERT INTO tags VALUES (1, 'transformer'), (2, 'auto');
         INSERT INTO itemTags VALUES (10, 1, 0), (10, 2, 1);

         INSERT INTO collections VALUES (1, 'Reading', 'EEEE5555', NULL),
                                        (2, 'Deep learning', 'FFFF6666', 1);
         INSERT INTO collectionItems VALUES (1, 10), (2, 10);",
    )
    .unwrap();
    drop(db);
    (dir, path)
}

#[test]
fn previews_a_library_without_reading_all_of_it() {
    let (_dir, path) = zotero_library();
    let seen = preview(&path).unwrap();
    // The attachment is not an item; the trashed one still counts as present.
    assert_eq!(seen, Preview { items: 3, collections: 2, tags: 2, attachments: 2, notes: 1 });
}

#[test]
fn refuses_a_file_that_is_not_a_zotero_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("other.sqlite");
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE x (y)").unwrap();

    let err = preview(&path).unwrap_err().to_string();
    assert!(err.contains("Zotero"), "says what is wrong: {err}");
}

#[test]
fn reads_items_with_their_fields() {
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();

    let paper = library
        .items
        .iter()
        .find(|i| i.key.as_ref().unwrap().as_str() == "AAAA1111")
        .expect("the item keeps its Zotero key");

    assert_eq!(paper.item_type, "journalArticle");
    assert_eq!(paper.fields["title"], "Attention Is All You Need");
    assert_eq!(paper.fields["DOI"], "10.1000/xyz");
}

#[test]
fn keeps_zotero_keys_so_a_second_import_updates_rather_than_duplicates() {
    let (_dir, path) = zotero_library();
    let first = read(&path).unwrap();
    let second = read(&path).unwrap();

    let keys = |l: &Imported| {
        let mut k: Vec<_> = l.items.iter().map(|i| i.key.clone().unwrap()).collect();
        k.sort_by_key(|k| k.to_string());
        k
    };
    assert_eq!(keys(&first), keys(&second));
}

#[test]
fn leaves_trashed_items_behind() {
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();
    assert!(
        !library.items.iter().any(|i| i.key.as_ref().unwrap().as_str() == "CCCC3333"),
        "an item Zotero put in the trash is not imported as a live one"
    );
    assert_eq!(library.items.len(), 2);
}

#[test]
fn reads_creators_in_order_and_understands_single_field_names() {
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();
    let paper = library.items.iter().find(|i| i.fields.contains_key("DOI")).unwrap();

    // Order matters: "first author" is a claim about position.
    assert_eq!(paper.creators.len(), 2);
    assert_eq!(paper.creators[0].last_name.as_deref(), Some("Vaswani"));
    assert_eq!(paper.creators[0].first_name.as_deref(), Some("Ashish"));

    // An organisation has one name, not a given and a family name.
    assert_eq!(paper.creators[1].name.as_deref(), Some("OpenAI"));
    assert_eq!(paper.creators[1].first_name, None);
}

#[test]
fn keeps_manual_and_automatic_tags_apart() {
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();
    let paper = library.items.iter().find(|i| i.fields.contains_key("DOI")).unwrap();

    let manual = paper.tags.iter().find(|t| t.tag == "transformer").unwrap();
    let automatic = paper.tags.iter().find(|t| t.tag == "auto").unwrap();
    assert_eq!(manual.r#type, 0);
    assert_eq!(automatic.r#type, 1);
}

#[test]
fn reads_the_collection_tree_and_its_membership() {
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();

    let child = library.collections.iter().find(|c| c.name == "Deep learning").unwrap();
    assert_eq!(child.parent.as_ref().map(Key::to_string).as_deref(), Some("EEEE5555"));

    assert_eq!(library.membership["AAAA1111"].len(), 2, "an item may be filed twice");
}

#[test]
fn never_writes_to_the_library_it_reads() {
    // It is very likely the user's only copy, and possibly open in Zotero right
    // now. Failing to import is recoverable; corrupting it is not.
    let (_dir, path) = zotero_library();
    let before = std::fs::metadata(&path).unwrap().len();
    read(&path).unwrap();
    preview(&path).unwrap();

    assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
    assert!(!path.with_extension("sqlite-wal").exists(), "read-only leaves no journal behind");
}

#[test]
fn finds_stored_files_and_leaves_linked_ones_alone() {
    let (dir, path) = zotero_library();
    // Put the file where Zotero would have.
    let stored = dir.path().join("storage").join("DDDD4444");
    std::fs::create_dir_all(&stored).unwrap();
    std::fs::write(stored.join("paper.pdf"), b"%PDF-1.7").unwrap();

    let library = read(&path).unwrap();
    assert_eq!(library.attachments.len(), 2);

    let imported = library.attachments.iter().find(|a| a.filename == "paper.pdf").unwrap();
    assert_eq!(imported.parent.as_str(), "AAAA1111");
    assert_eq!(imported.content_type, "application/pdf");
    assert!(imported.source.as_deref().is_some_and(|p| p.is_file()), "the bytes were found");

    // A linked file lives somewhere this machine may not even have, and is not
    // ours to copy.
    let linked = library.attachments.iter().find(|a| a.filename == "linked.pdf").unwrap();
    assert_eq!(linked.source, None);
}

#[test]
fn an_attachment_whose_file_is_missing_is_still_recorded() {
    // Zotero's own storage can be incomplete after a partial sync. The record
    // is worth keeping; it says what the item is missing.
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();

    let imported = library.attachments.iter().find(|a| a.filename == "paper.pdf").unwrap();
    assert_eq!(imported.source, None, "no bytes, but the attachment is known");
}

#[test]
fn reads_a_library_that_predates_the_content_type_column() {
    // Zotero added `contentType` along the way. An older library must still
    // import — and, more importantly, asking for the column by name means a
    // mistake anywhere else in the statement is still reported.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zotero.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT);
         CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INTEGER, key TEXT,
                             dateAdded TEXT, dateModified TEXT);
         CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY);
         CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT);
         CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value TEXT);
         CREATE TABLE itemData (itemID INTEGER, fieldID INTEGER, valueID INTEGER);
         CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT,
                                fieldMode INTEGER);
         CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
         CREATE TABLE itemCreators (itemID INTEGER, creatorID INTEGER, creatorTypeID INTEGER,
                                    orderIndex INTEGER);
         CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE itemTags (itemID INTEGER, tagID INTEGER, type INTEGER);
         CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT,
                                   key TEXT, parentCollectionID INTEGER);
         CREATE TABLE collectionItems (collectionID INTEGER, itemID INTEGER);
         -- No contentType here.
         CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INTEGER, path TEXT);
         INSERT INTO itemTypes VALUES (1, 'journalArticle'), (2, 'attachment');
         INSERT INTO fields VALUES (1, 'title');
         INSERT INTO items VALUES (1, 1, 'OLDL1111', '', ''), (2, 2, 'OLDA2222', '', '');
         INSERT INTO itemAttachments VALUES (2, 1, 'storage:old.pdf');",
    )
    .unwrap();
    drop(db);

    let library = read(&path).unwrap();
    assert_eq!(library.attachments.len(), 1);
    assert_eq!(library.attachments[0].content_type, "application/pdf", "a sensible default");
}

#[test]
fn brings_the_users_own_notes_across() {
    // Somebody's notes about a paper exist nowhere else. Dropping them would
    // make the import lossy in exactly the way that matters most.
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();

    assert_eq!(library.notes.len(), 1);
    assert_eq!(library.notes[0].parent.as_str(), "AAAA1111");
    assert!(library.notes[0].html.contains("attention"));
}

#[test]
fn leaves_a_note_with_no_item_where_it_is() {
    // A standalone note belongs at the top level, and there is nowhere for it
    // here yet. Leaving it in Zotero beats filing it where nobody will look.
    let (_dir, path) = zotero_library();
    let library = read(&path).unwrap();
    assert!(!library.notes.iter().any(|n| n.html.contains("standalone")));
}

#[test]
fn reads_a_library_with_no_notes_table_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zotero.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT);
         CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INTEGER, key TEXT,
                             dateAdded TEXT, dateModified TEXT);
         CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY);
         CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT);
         CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value TEXT);
         CREATE TABLE itemData (itemID INTEGER, fieldID INTEGER, valueID INTEGER);
         CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT,
                                fieldMode INTEGER);
         CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
         CREATE TABLE itemCreators (itemID INTEGER, creatorID INTEGER, creatorTypeID INTEGER,
                                    orderIndex INTEGER);
         CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE itemTags (itemID INTEGER, tagID INTEGER, type INTEGER);
         CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT,
                                   key TEXT, parentCollectionID INTEGER);
         CREATE TABLE collectionItems (collectionID INTEGER, itemID INTEGER);
         CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INTEGER, path TEXT);
         INSERT INTO itemTypes VALUES (1, 'journalArticle');
         INSERT INTO fields VALUES (1, 'title');
         INSERT INTO items VALUES (1, 1, 'BARE1111', '', '');",
    )
    .unwrap();
    drop(db);

    let library = read(&path).unwrap();
    assert_eq!(library.items.len(), 1, "the items still arrive");
    assert!(library.notes.is_empty());
}
