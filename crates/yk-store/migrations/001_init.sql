-- Yinkote schema v1.
-- Design notes:
--   * `items` is the single table for every item kind (regular / note /
--     attachment); `item_type` discriminates. This keeps versioning, tagging
--     and trash uniform.
--   * Fields and creators live in JSON columns for schema-driven flexibility;
--     the columns used for sorting and filtering are denormalised alongside so
--     every list query stays index-backed.
--   * Two FTS5 tables: `items_fts` for BM25 ranking (fed pre-tokenised text so
--     CJK works without an external dictionary) and `items_trgm` for
--     substring / typo-tolerant candidate generation.

CREATE TABLE libraries (
    id         INTEGER PRIMARY KEY,
    type       TEXT    NOT NULL DEFAULT 'user',
    name       TEXT    NOT NULL,
    version    INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE TABLE collections (
    id         INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL,
    parent_id  INTEGER REFERENCES collections(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    sort_index REAL    NOT NULL DEFAULT 0,
    version    INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX idx_collections_key    ON collections(library_id, key);
CREATE INDEX        idx_collections_parent ON collections(library_id, parent_id, sort_index);

CREATE TABLE items (
    id            INTEGER PRIMARY KEY,
    library_id    INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    key           TEXT    NOT NULL,
    item_type     TEXT    NOT NULL,
    parent_id     INTEGER REFERENCES items(id) ON DELETE CASCADE,
    fields        TEXT    NOT NULL DEFAULT '{}',
    creators      TEXT    NOT NULL DEFAULT '[]',
    sort_title    TEXT    NOT NULL DEFAULT '',
    sort_creator  TEXT    NOT NULL DEFAULT '',
    year          INTEGER,
    fingerprint   TEXT    NOT NULL DEFAULT '',
    deleted       INTEGER NOT NULL DEFAULT 0,
    version       INTEGER NOT NULL DEFAULT 0,
    date_added    INTEGER NOT NULL,
    date_modified INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_items_key      ON items(library_id, key);
CREATE INDEX idx_items_modified        ON items(library_id, deleted, date_modified DESC);
CREATE INDEX idx_items_added           ON items(library_id, deleted, date_added DESC);
CREATE INDEX idx_items_title           ON items(library_id, deleted, sort_title);
CREATE INDEX idx_items_creator         ON items(library_id, deleted, sort_creator);
CREATE INDEX idx_items_year            ON items(library_id, deleted, year);
CREATE INDEX idx_items_type            ON items(library_id, deleted, item_type);
CREATE INDEX idx_items_version         ON items(library_id, version);
CREATE INDEX idx_items_fingerprint     ON items(library_id, fingerprint);
CREATE INDEX idx_items_parent          ON items(parent_id);

CREATE TABLE tags (
    id         INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    color      TEXT,
    position   INTEGER
);
CREATE UNIQUE INDEX idx_tags_name ON tags(library_id, name);

CREATE TABLE item_tags (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    type    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX idx_item_tags_tag ON item_tags(tag_id);

CREATE TABLE collection_items (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    item_id       INTEGER NOT NULL REFERENCES items(id)       ON DELETE CASCADE,
    sort_index    REAL    NOT NULL DEFAULT 0,
    PRIMARY KEY (collection_id, item_id)
) WITHOUT ROWID;
CREATE INDEX idx_collection_items_item ON collection_items(item_id);

-- Tombstones for delta sync.
CREATE TABLE deletions (
    library_id  INTEGER NOT NULL,
    object_type TEXT    NOT NULL,
    object_key  TEXT    NOT NULL,
    version     INTEGER NOT NULL,
    deleted_at  INTEGER NOT NULL,
    PRIMARY KEY (library_id, object_type, object_key)
) WITHOUT ROWID;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- BM25 index. `body` receives text already tokenised by the host, which is how
-- Chinese/Japanese/Korean search works without bundling a dictionary.
CREATE VIRTUAL TABLE items_fts USING fts5(
    title, creators, body, tags,
    tokenize = "unicode61 remove_diacritics 2"
);

-- Substring / typo-tolerant candidate generation.
CREATE VIRTUAL TABLE items_trgm USING fts5(
    text,
    tokenize = "trigram"
);

-- Dense vectors for semantic search.
CREATE TABLE item_vectors (
    item_id      INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    library_id   INTEGER NOT NULL,
    provider     TEXT    NOT NULL,
    dim          INTEGER NOT NULL,
    content_hash TEXT    NOT NULL,
    vec          BLOB    NOT NULL
);
CREATE INDEX idx_item_vectors_lib ON item_vectors(library_id);

-- Documents awaiting embedding.
CREATE TABLE embed_queue (
    item_id      INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    library_id   INTEGER NOT NULL,
    text         TEXT    NOT NULL,
    content_hash TEXT    NOT NULL,
    queued_at    INTEGER NOT NULL
);
