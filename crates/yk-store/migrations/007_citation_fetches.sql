-- When a paper's reference list was last asked for.
--
-- Not derivable from `item_relations`, and that is the point: a great many
-- papers have *no* deposited references, and for those "we asked and got
-- nothing" and "we never asked" produce exactly the same absence of rows. Told
-- apart only by this table — without it, every bulk run asks Crossref about
-- every one of them again, forever, which is both slow and rude to a free
-- service.
CREATE TABLE citation_fetches (
    item_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,
    fetched_at INTEGER NOT NULL,
    found      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, kind)
) WITHOUT ROWID;
