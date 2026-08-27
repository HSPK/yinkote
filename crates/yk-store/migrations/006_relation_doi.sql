-- The identifier as the publisher wrote it, beside the normalised form used
-- for matching.
--
-- The fingerprint flattens punctuation so that two spellings of one DOI match,
-- which makes it a one-way road: `10.1016/j.cell.2020.01.001` normalises to a
-- run of words that cannot be put back together. Anything that has to *use*
-- the identifier — fetching the metadata for a work the library is missing —
-- needs the original, so it is kept rather than reconstructed by guesswork.
ALTER TABLE item_relations ADD COLUMN target_doi TEXT NOT NULL DEFAULT '';
