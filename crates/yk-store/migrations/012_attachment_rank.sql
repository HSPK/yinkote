-- Sorting by what a row has attached.
--
-- The rank is stored rather than worked out on demand for the reason every
-- other sortable value here is: a correlated subquery in ORDER BY costs the
-- whole library on every page. Measured on 130k items it was 109ms against
-- 9ms for the indexed sorts, and it grows with the library rather than with
-- the page.
--
-- Higher is better, so a descending sort puts papers with a PDF first and
-- papers with nothing last, which is what somebody clicking this column is
-- looking for. The scale matches `AttachmentKind::ORDER` in yk-core.
ALTER TABLE items ADD COLUMN attachment_rank INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_items_attachment
    ON items(library_id, deleted, attachment_rank DESC, id DESC);

-- The rank of one attachment row, by what it holds.
--
-- Repeated in each trigger below because SQLite has no scalar functions in
-- schema definitions; `yk_store::attachment_rank_sql` holds the one copy that
-- Rust uses, and a test asserts the two agree.
--
--   4 pdf · 3 saved page · 2 link only · 1 some other file · 0 nothing

UPDATE items SET attachment_rank = coalesce((
    SELECT max(CASE
        WHEN json_extract(a.fields, '$.linkMode') = 'linked_url' THEN 2
        WHEN json_extract(a.fields, '$.contentType') = 'application/pdf' THEN 4
        WHEN json_extract(a.fields, '$.contentType') IN ('text/html', 'application/xhtml+xml') THEN 3
        ELSE 1 END)
    FROM items a
    WHERE a.parent_id = items.id AND a.deleted = 0 AND a.item_type = 'attachment'
), 0);

-- Kept up to date by the database rather than by every write path that touches
-- an attachment. There are five of those and counting — create, batch create,
-- patch, trash, merge — and the one that gets forgotten is invisible: the
-- column simply goes stale and a column sorts wrongly for a library nobody is
-- looking at yet.

-- Incremental, not a recompute: a new child can only raise the maximum, so the
-- answer is `max(what it was, what this one is worth)`.
--
-- Recomputing here instead is correct and quadratic. Importing a paper with a
-- thousand attachments meant the thousandth insert scanned nine hundred and
-- ninety-nine siblings, and a test that had run in a second took over a minute.
CREATE TRIGGER items_attachment_rank_insert
AFTER INSERT ON items
WHEN new.parent_id IS NOT NULL AND new.item_type = 'attachment' AND new.deleted = 0
BEGIN
    UPDATE items SET attachment_rank = max(attachment_rank, CASE
        WHEN json_extract(new.fields, '$.linkMode') = 'linked_url' THEN 2
        WHEN json_extract(new.fields, '$.contentType') = 'application/pdf' THEN 4
        WHEN json_extract(new.fields, '$.contentType') IN ('text/html', 'application/xhtml+xml') THEN 3
        ELSE 1 END)
    WHERE id = new.parent_id;
END;

-- Both parents, because a merge moves a file from one paper to another and
-- the one it left has to stop claiming it.
-- A change can lower the maximum — a PDF retyped as a link, a file trashed —
-- so this one has to look at the siblings. It is the rare direction: files are
-- added in bulk and edited one at a time.
CREATE TRIGGER items_attachment_rank_update
AFTER UPDATE ON items
WHEN (old.parent_id IS NOT NULL OR new.parent_id IS NOT NULL)
  AND (old.item_type = 'attachment' OR new.item_type = 'attachment')
BEGIN
    UPDATE items SET attachment_rank = coalesce((
        SELECT max(CASE
            WHEN json_extract(a.fields, '$.linkMode') = 'linked_url' THEN 2
            WHEN json_extract(a.fields, '$.contentType') = 'application/pdf' THEN 4
            WHEN json_extract(a.fields, '$.contentType') IN ('text/html', 'application/xhtml+xml') THEN 3
            ELSE 1 END)
        FROM items a
        WHERE a.parent_id = items.id AND a.deleted = 0 AND a.item_type = 'attachment'
    ), 0)
    WHERE id IN (old.parent_id, new.parent_id);
END;

CREATE TRIGGER items_attachment_rank_delete
AFTER DELETE ON items
WHEN old.parent_id IS NOT NULL AND old.item_type = 'attachment'
BEGIN
    UPDATE items SET attachment_rank = coalesce((
        SELECT max(CASE
            WHEN json_extract(a.fields, '$.linkMode') = 'linked_url' THEN 2
            WHEN json_extract(a.fields, '$.contentType') = 'application/pdf' THEN 4
            WHEN json_extract(a.fields, '$.contentType') IN ('text/html', 'application/xhtml+xml') THEN 3
            ELSE 1 END)
        FROM items a
        WHERE a.parent_id = old.parent_id AND a.deleted = 0 AND a.item_type = 'attachment'
    ), 0) WHERE id = old.parent_id;
END;
