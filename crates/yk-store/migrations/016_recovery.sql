CREATE TABLE file_cleanup (
    library_id INTEGER NOT NULL,
    item_key TEXT NOT NULL,
    PRIMARY KEY (library_id, item_key)
) WITHOUT ROWID;

CREATE TABLE task_records (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL
) WITHOUT ROWID;

-- Cascaded children need the same cleanup as explicitly deleted parents.
CREATE TRIGGER items_cleanup BEFORE DELETE ON items BEGIN
    INSERT OR IGNORE INTO file_cleanup(library_id, item_key) VALUES (OLD.library_id, OLD.key);
    DELETE FROM items_fts WHERE rowid = OLD.id;
    DELETE FROM items_trgm WHERE rowid = OLD.id;
    DELETE FROM fetch_queue WHERE library_id = OLD.library_id AND item_key = OLD.key;
END;
