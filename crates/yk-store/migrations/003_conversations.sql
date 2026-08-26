-- Conversation history.
--
-- Kept in the library database rather than browser storage so a conversation
-- survives a restart, follows the user between browsers, and can later be
-- referenced by an agent alongside the items it cited.

CREATE TABLE conversations (
    id         INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL,
    title      TEXT    NOT NULL,
    /** What the conversation was scoped to, e.g. a collection key. */
    scope      TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_conversations_key ON conversations(library_id, key);
CREATE INDEX idx_conversations_recent ON conversations(library_id, updated_at DESC);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT    NOT NULL CHECK (role IN ('user', 'assistant', 'tool', 'system')),
    content         TEXT    NOT NULL,
    -- Tool calls, cited item keys, token usage: anything the UI renders beside
    -- the text but that is not part of it.
    meta            TEXT,
    created_at      INTEGER NOT NULL
);
CREATE INDEX idx_messages_conversation ON messages(conversation_id, id);
