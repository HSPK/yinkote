-- Collections have carried no timestamps since 001, so the browser could sort
-- them by name, kind and size but not by when they appeared — which is the one
-- ordering that answers "what was I doing last week".
--
-- Items have had `date_added`/`date_modified` all along; collections get the
-- same two columns with the same names and the same millisecond epoch, so a
-- reader who knows one table knows the other.
--
-- Existing rows have no honest answer: nothing recorded when they were made.
-- They are left at 0, which the interface shows as "unknown" rather than
-- inventing today's date and quietly making every old collection look new.
ALTER TABLE collections ADD COLUMN date_added    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE collections ADD COLUMN date_modified INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_collections_added ON collections(library_id, date_added DESC);

-- Smart collections sit in the same browser under the same "created" column.
-- Dating one kind and not the other would make the column look broken for
-- every second row, so both get it.
ALTER TABLE smart_collections ADD COLUMN date_added    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE smart_collections ADD COLUMN date_modified INTEGER NOT NULL DEFAULT 0;
