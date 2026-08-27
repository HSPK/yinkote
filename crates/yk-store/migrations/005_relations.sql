-- What a paper cites.
--
-- The graph's other edges — shared tags, shared authors, shared shelves — are
-- derived, because the items already imply them. A citation is not: it is a
-- fact from the publisher, it exists whether or not either paper is in this
-- library, and there is nowhere else to keep it. That is the line: derive what
-- the library implies, store what the world tells you.
--
-- The cited work is written down as a *fingerprint*, the same shape
-- `Item::fingerprint` produces, rather than as a foreign key. Two reasons, and
-- the second is the important one:
--
--   * Most cited works are not in the library, and a foreign key cannot point
--     at a paper nobody owns.
--   * Resolution happens when the graph is read, not when the reference is
--     stored. So adding the cited paper later turns the edge into an internal
--     one by itself — no backfill, nothing to go stale, no moment where the
--     library holds both papers and still draws them as strangers.
CREATE TABLE item_relations (
    source_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL,
    -- Order in the source's bibliography. A reference list is numbered in the
    -- paper it came from; renumbering it is quiet damage.
    position     INTEGER NOT NULL,
    -- `doi:...` when the publisher deposited one, empty when they did not.
    target_key   TEXT    NOT NULL DEFAULT '',
    target_label TEXT    NOT NULL DEFAULT '',
    target_year  INTEGER,
    PRIMARY KEY (source_id, kind, position)
) WITHOUT ROWID;

-- "Who cites this?" is the same question backwards, and is asked just as often.
CREATE INDEX idx_relations_target ON item_relations(target_key, kind);
