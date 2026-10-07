CREATE INDEX idx_embed_queue_order ON embed_queue(queued_at, item_id);
CREATE INDEX idx_item_vectors_provider ON item_vectors(provider, dim);
