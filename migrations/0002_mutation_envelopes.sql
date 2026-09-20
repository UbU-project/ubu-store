-- Canonical payloads are stored in full, duplicating payload storage for each
-- mutation. A later ticket may replace this text with a digest; Phase 1b does
-- not add a hashing dependency.
CREATE TABLE mutation_envelopes (
    origin_device_id  TEXT NOT NULL,
    idempotency_key   TEXT NOT NULL,
    envelope_json    TEXT NOT NULL,
    canonical_payload TEXT NOT NULL,
    result_object_id TEXT NOT NULL,
    result_version   INTEGER NOT NULL,
    recorded_at      TEXT NOT NULL,
    PRIMARY KEY (origin_device_id, idempotency_key)
);
CREATE INDEX idx_mutation_envelopes_object ON mutation_envelopes (result_object_id);
