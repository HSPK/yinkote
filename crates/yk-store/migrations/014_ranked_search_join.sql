-- Ranked keyword search pays a full row lookup for every match it scores.
--
-- `bm25()` in an `ORDER BY` has to score every matching row before it can know
-- which three hundred to keep, and each of those rows crosses into `items` to
-- check the library and the trash. On this library "transformer" matches
-- 20,146 documents, so a search that returns 300 rows visits 20,146 of them --
-- and reads the whole row each time, for two integers.
--
-- The same query without ranking costs 3.5ms, because it stops at the limit
-- and never scores anything; the difference is not the scoring but the twenty
-- thousand table lookups that scoring forces. This index carries exactly the
-- two columns the join tests, so the lookup is answered from the index and the
-- table is never touched: 44.4ms -> 31.2ms, with no change to what is
-- returned.
--
-- Not partial, unlike `idx_items_attachment_added`: every item is a candidate
-- for a keyword search, so there is no subset to restrict it to. It is three
-- integers per row and it is only read, never sorted by.
CREATE INDEX IF NOT EXISTS idx_items_live
    ON items(id, library_id, deleted);
