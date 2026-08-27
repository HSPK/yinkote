-- Files waiting to be downloaded.
--
-- A queue rather than a request because downloading is slow, fallible and
-- worth retrying: a paper behind a slow publisher should not hold up the click
-- that asked for it, and a failure at three in the morning should still be
-- there to retry in the morning. That is also why it is a table and not a
-- channel — a queue that forgets everything when the process restarts is not a
-- queue, it is a buffer.
--
-- One row per (item, url). Asking twice for the same file is the same request,
-- not two, and double-clicking a button is the commonest way it happens.
CREATE TABLE fetch_queue (
    id         INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    item_key   TEXT    NOT NULL,
    url        TEXT    NOT NULL,
    -- waiting | running | done | failed
    state      TEXT    NOT NULL DEFAULT 'waiting',
    attempts   INTEGER NOT NULL DEFAULT 0,
    -- Kept rather than logged: the reason a download failed is the thing the
    -- user needs in order to decide whether retrying is worth anything.
    error      TEXT    NOT NULL DEFAULT '',
    title      TEXT    NOT NULL DEFAULT '',
    bytes      INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_fetch_queue_target ON fetch_queue(library_id, item_key, url);
CREATE INDEX idx_fetch_queue_state ON fetch_queue(library_id, state, id);
