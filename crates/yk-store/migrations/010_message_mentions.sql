-- Which papers a message is about.
--
-- Derived from the message the moment it is written, in the same transaction,
-- because the alternative is a table that can disagree with the conversation
-- it describes.
--
-- The point of the index is the reverse lookup: standing on a paper, what have
-- I already asked about it? That question is asked from the item's detail
-- panel, so it has to be answerable without scanning every message ever sent.

CREATE TABLE message_mentions (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    item_key   TEXT    NOT NULL,
    PRIMARY KEY (message_id, item_key)
);

CREATE INDEX idx_message_mentions_item ON message_mentions(item_key);
