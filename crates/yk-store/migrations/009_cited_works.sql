-- How often each cited work is cited, maintained rather than counted.
--
-- Counting it on demand meant aggregating every stored reference — 1.8 million
-- rows in a hundred-thousand-item library — on every visit to a browsing page.
-- Measured: over ten minutes in its first shape, 2.8 seconds after the query
-- was restructured. Neither is a page.
--
-- This is a *derived* table, which the rest of the project avoids: the rule has
-- been "derive what the library implies, store what the world tells you". The
-- exception is earned the same way the search index earns it — the count is
-- updated inside the same transaction as the references it counts, so it
-- cannot disagree with them. A cache that can drift is the thing worth
-- refusing; one that is written with its source is just an index.
CREATE TABLE cited_works (
    -- `doi:…`, the same shape `Item::fingerprint` produces.
    target_key TEXT PRIMARY KEY,
    label      TEXT    NOT NULL DEFAULT '',
    year       INTEGER,
    doi        TEXT    NOT NULL DEFAULT '',
    -- How many distinct papers in the library cite it.
    citations  INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

-- The whole point: "what is cited most" is a scan of this index, not of the
-- references.
CREATE INDEX idx_cited_works_rank ON cited_works(citations DESC);

-- Fill it from what is already stored, so an existing library does not have to
-- re-fetch every bibliography to get the feature.
INSERT INTO cited_works (target_key, label, year, doi, citations)
SELECT target_key,
       max(target_label),
       max(target_year),
       max(target_doi),
       count(DISTINCT source_id)
FROM item_relations
WHERE kind = 'cites' AND target_key != ''
GROUP BY target_key;
