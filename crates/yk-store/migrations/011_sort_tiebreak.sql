-- Sort indexes that include the tiebreaker.
--
-- Every list is ordered by a column *and then by id*, so that paging is stable
-- when two items share a date or a title. The indexes stopped at the sort
-- column, so SQLite could use them to find the rows but had to build a temp
-- b-tree to settle the ties — and it did that over every row the query
-- touched, including the fifty thousand that an offset was about to discard.
--
-- Measured on a 100k library, page at offset 50000: 95.7ms with the old
-- indexes, 23.5ms with these. The rest of that number is dealt with by the
-- deferred join in `items.rs`; together they make it 2.1ms.
--
-- The direction only has to be *consistent*: SQLite reads an index backwards
-- when the query asks for the reverse, and `ORDER BY x DESC, id DESC` is the
-- exact reverse of `(x ASC, id ASC)`.

DROP INDEX IF EXISTS idx_items_modified;
DROP INDEX IF EXISTS idx_items_added;
DROP INDEX IF EXISTS idx_items_title;
DROP INDEX IF EXISTS idx_items_creator;
DROP INDEX IF EXISTS idx_items_year;
DROP INDEX IF EXISTS idx_items_type;

CREATE INDEX idx_items_modified ON items(library_id, deleted, date_modified DESC, id DESC);
CREATE INDEX idx_items_added    ON items(library_id, deleted, date_added DESC, id DESC);
CREATE INDEX idx_items_title    ON items(library_id, deleted, sort_title, id);
CREATE INDEX idx_items_creator  ON items(library_id, deleted, sort_creator, id);
CREATE INDEX idx_items_year     ON items(library_id, deleted, year, id);
CREATE INDEX idx_items_type     ON items(library_id, deleted, item_type, id);
