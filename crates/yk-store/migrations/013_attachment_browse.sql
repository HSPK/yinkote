-- The file browser walks the whole library to find the files in it.
--
-- `attachments()` filters on `item_type = 'attachment'` and orders by
-- `date_added`, and `idx_items_added` carries the order but not the type. So
-- SQLite drove down the date index and tested every row: 99,839 items visited
-- to return 286 attachments, 48ms for one page, on the screen whose entire job
-- is listing files.
--
-- Naming `idx_items_type` instead fixes this library (0.2ms) and loses on the
-- opposite one, where most items are attachments and the sort is then over
-- tens of thousands of rows. A partial index has neither problem: it holds
-- only attachment rows, in the order the query wants, so the cost is the size
-- of the page and nothing else. Measured 48ms -> 0.1ms here, and it is
-- covering, so there is no table lookup either.
--
-- Cheap because it is partial: attachments are a small minority of any library
-- (one file per paper at most), and the index is empty for everything else.
CREATE INDEX IF NOT EXISTS idx_items_attachment_added
    ON items(library_id, deleted, date_added DESC, id DESC)
    WHERE item_type = 'attachment';
