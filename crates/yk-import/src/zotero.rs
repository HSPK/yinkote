//! Reading a Zotero library.
//!
//! Zotero keeps everything in a `zotero.sqlite` beside its storage directory,
//! and its schema is close enough to this one that the mapping is mostly a
//! rename: both model an item as a type plus a bag of named fields, both give
//! items an eight-character key, and this project's item types were drawn from
//! Zotero's in the first place.
//!
//! Two rules shape everything here.
//!
//! **The file is opened read-only and never written.** It is the user's other
//! library, very likely their only copy, and possibly open in Zotero at this
//! moment. An import that could corrupt it would be unforgivable in a way that
//! failing to import simply is not.
//!
//! **Zotero's keys are kept.** They are unique, stable, and already the way
//! Zotero's own sync identifies an item — so importing the same library twice
//! updates rather than duplicates, and a library imported here stays
//! recognisable to anything that knew it there.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use yk_core::model::{Creator, ItemDraft, ItemTag};
use yk_core::{Error, Key, Result};

/// What a library holds, without reading all of it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Preview {
    pub items: i64,
    pub collections: i64,
    pub tags: i64,
    /// Attachments whose files live in Zotero's storage directory.
    pub attachments: i64,
}

/// A collection, with its parent's key when it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedCollection {
    pub key: Key,
    pub name: String,
    pub parent: Option<Key>,
}

/// Everything read out of a Zotero library.
pub struct Imported {
    pub items: Vec<ItemDraft>,
    pub collections: Vec<ImportedCollection>,
    /// Item key to the collection keys holding it.
    pub membership: HashMap<String, Vec<Key>>,
}

/// Zotero field names this project does not have, and what they become here.
///
/// Everything else passes through unchanged, because both schemas took their
/// names from the same place. Anything genuinely unknown is dropped rather than
/// stored under a name nothing will look for.
const FIELD_ALIASES: &[(&str, &str)] = &[
    ("publicationTitle", "publicationTitle"),
    ("date", "date"),
    ("DOI", "DOI"),
    ("ISBN", "ISBN"),
    ("ISSN", "ISSN"),
    ("url", "url"),
    ("abstractNote", "abstractNote"),
    ("extra", "extra"),
];

/// Open a Zotero database without any possibility of altering it.
fn open(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    Connection::open_with_flags(path, flags)
        .map_err(|e| Error::invalid(format!("cannot read {}: {e}", path.display())))
}

/// Confirm this really is a Zotero library before anything else happens.
fn check_schema(db: &Connection) -> Result<()> {
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' \
             AND name IN ('items','itemData','itemDataValues','fields','itemTypes')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| Error::invalid(format!("not a readable database: {e}")))?;

    if tables < 5 {
        return Err(Error::invalid("this does not look like a Zotero library"));
    }
    Ok(())
}

/// Count what a library holds, so the user can be told before committing.
pub fn preview(path: &Path) -> Result<Preview> {
    let db = open(path)?;
    check_schema(&db)?;

    let count = |sql: &str| -> i64 { db.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
    Ok(Preview {
        items: count(
            "SELECT count(*) FROM items i JOIN itemTypes t ON t.itemTypeID = i.itemTypeID \
             WHERE t.typeName NOT IN ('attachment','note','annotation')",
        ),
        collections: count("SELECT count(*) FROM collections"),
        tags: count("SELECT count(*) FROM tags"),
        attachments: count("SELECT count(*) FROM itemAttachments WHERE path IS NOT NULL"),
    })
}

/// Read a whole library.
pub fn read(path: &Path) -> Result<Imported> {
    let db = open(path)?;
    check_schema(&db)?;

    let collections = read_collections(&db)?;
    let fields = read_fields(&db)?;
    let creators = read_creators(&db)?;
    let tags = read_tags(&db)?;
    let membership = read_membership(&db)?;

    let mut items = Vec::new();
    let mut stmt = db
        .prepare(
            "SELECT i.itemID, i.key, t.typeName, i.dateAdded, i.dateModified \
             FROM items i JOIN itemTypes t ON t.itemTypeID = i.itemTypeID \
             WHERE t.typeName NOT IN ('attachment','note','annotation') \
             AND i.itemID NOT IN (SELECT itemID FROM deletedItems)",
        )
        .map_err(sql)?;

    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(sql)?;

    for row in rows {
        let (id, key, item_type) = row.map_err(sql)?;
        // A key Zotero considers valid but this project does not is not worth
        // losing the item over; a fresh one keeps everything else.
        let key = Key::parse(&key).unwrap_or_else(|_| Key::generate());

        let mut draft = ItemDraft::new(&item_type);
        draft.key = Some(key);
        for (name, value) in fields.get(&id).into_iter().flatten() {
            if let Some(mapped) = map_field(name) {
                draft.fields.insert(mapped.to_string(), serde_json::json!(value));
            }
        }
        draft.creators = creators.get(&id).cloned().unwrap_or_default();
        draft.tags = tags.get(&id).cloned().unwrap_or_default();
        items.push(draft);
    }

    Ok(Imported { items, collections, membership })
}

fn map_field(name: &str) -> Option<&str> {
    FIELD_ALIASES
        .iter()
        .find(|(from, _)| *from == name)
        .map(|(_, to)| *to)
        // Both schemas name their fields the same way, so an unlisted field is
        // almost certainly fine as-is; only obviously internal ones are cut.
        .or(Some(name).filter(|n| !n.starts_with("__")))
}

fn read_collections(db: &Connection) -> Result<Vec<ImportedCollection>> {
    let mut stmt = db
        .prepare(
            "SELECT c.key, c.collectionName, p.key \
             FROM collections c LEFT JOIN collections p ON p.collectionID = c.parentCollectionID",
        )
        .map_err(sql)?;

    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
        })
        .map_err(sql)?;

    let mut out = Vec::new();
    for row in rows {
        let (key, name, parent) = row.map_err(sql)?;
        out.push(ImportedCollection {
            key: Key::parse(&key).unwrap_or_else(|_| Key::generate()),
            name,
            parent: parent.and_then(|p| Key::parse(&p).ok()),
        });
    }
    Ok(out)
}

/// `itemID` to its field name/value pairs.
fn read_fields(db: &Connection) -> Result<HashMap<i64, Vec<(String, String)>>> {
    let mut stmt = db
        .prepare(
            "SELECT d.itemID, f.fieldName, v.value FROM itemData d \
             JOIN fields f ON f.fieldID = d.fieldID \
             JOIN itemDataValues v ON v.valueID = d.valueID",
        )
        .map_err(sql)?;

    let mut out: HashMap<i64, Vec<(String, String)>> = HashMap::new();
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })
        .map_err(sql)?;
    for row in rows {
        let (id, name, value) = row.map_err(sql)?;
        out.entry(id).or_default().push((name, value));
    }
    Ok(out)
}

fn read_creators(db: &Connection) -> Result<HashMap<i64, Vec<Creator>>> {
    // Zotero 5 and later keep names on `creators`; the ordering column is what
    // makes "first author" mean anything, so it decides the order here too.
    let mut stmt = db
        .prepare(
            "SELECT ic.itemID, c.firstName, c.lastName, c.fieldMode, ct.creatorType \
             FROM itemCreators ic \
             JOIN creators c ON c.creatorID = ic.creatorID \
             JOIN creatorTypes ct ON ct.creatorTypeID = ic.creatorTypeID \
             ORDER BY ic.itemID, ic.orderIndex",
        )
        .map_err(sql)?;

    let mut out: HashMap<i64, Vec<Creator>> = HashMap::new();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(sql)?;

    for row in rows {
        let (id, first, last, field_mode, creator_type) = row.map_err(sql)?;
        // Field mode 1 means a single-field name — an organisation, or a name
        // that does not split into given and family.
        let creator = if field_mode == 1 {
            Creator {
                creator_type,
                name: last.clone().or(first),
                ..Default::default()
            }
        } else {
            Creator { creator_type, first_name: first, last_name: last, ..Default::default() }
        };
        out.entry(id).or_default().push(creator);
    }
    Ok(out)
}

fn read_tags(db: &Connection) -> Result<HashMap<i64, Vec<ItemTag>>> {
    let mut stmt = db
        .prepare(
            "SELECT it.itemID, t.name, it.type FROM itemTags it JOIN tags t ON t.tagID = it.tagID",
        )
        .map_err(sql)?;

    let mut out: HashMap<i64, Vec<ItemTag>> = HashMap::new();
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .map_err(sql)?;
    for row in rows {
        let (id, name, kind) = row.map_err(sql)?;
        // Zotero uses the same convention: 0 is the user's own, 1 automatic.
        out.entry(id).or_default().push(ItemTag { tag: name, r#type: kind as u8 });
    }
    Ok(out)
}

fn read_membership(db: &Connection) -> Result<HashMap<String, Vec<Key>>> {
    let mut stmt = db
        .prepare(
            "SELECT i.key, c.key FROM collectionItems ci \
             JOIN items i ON i.itemID = ci.itemID \
             JOIN collections c ON c.collectionID = ci.collectionID",
        )
        .map_err(sql)?;

    let mut out: HashMap<String, Vec<Key>> = HashMap::new();
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(sql)?;
    for row in rows {
        let (item, collection) = row.map_err(sql)?;
        if let Ok(key) = Key::parse(&collection) {
            out.entry(item).or_default().push(key);
        }
    }
    Ok(out)
}

fn sql(e: rusqlite::Error) -> Error {
    Error::invalid(format!("reading the Zotero library: {e}"))
}

#[cfg(test)]
mod tests;
