-- Smart collections.
--
-- A smart collection is a *saved query*, not a second membership mechanism.
-- It stores the same search string the user types into the search box, so the
-- query language, the retrieval pipeline and the filters are shared verbatim —
-- there is no second evaluation engine to keep in step with the first.

CREATE TABLE smart_collections (
    id         INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    -- Exactly what would be typed into the search box, e.g.
    -- `扩散模型 tag:综述 -tag:废弃 year:2020..2024`.
    query      TEXT    NOT NULL DEFAULT '',
    mode       TEXT    NOT NULL DEFAULT 'hybrid',
    sort       TEXT    NOT NULL DEFAULT 'dateModified',
    direction  TEXT    NOT NULL DEFAULT 'desc',
    sort_index REAL    NOT NULL DEFAULT 0,
    version    INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX idx_smart_key ON smart_collections(library_id, key);
