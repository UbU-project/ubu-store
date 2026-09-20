CREATE TABLE advisory_candidates (
    advisory_candidate_id TEXT PRIMARY KEY,
    candidate_kind        TEXT    NOT NULL,
    lifecycle_state       TEXT    NOT NULL,
    version               INTEGER NOT NULL,
    suppression_key       TEXT,
    review_order          INTEGER,
    payload_json          TEXT    NOT NULL,
    created_at            TEXT    NOT NULL,
    updated_at            TEXT    NOT NULL
);
CREATE INDEX idx_advisory_candidates_queue ON advisory_candidates (lifecycle_state, review_order);
CREATE INDEX idx_advisory_candidates_suppression ON advisory_candidates (suppression_key);

CREATE TABLE candidate_decision_events (
    id                         TEXT PRIMARY KEY,
    advisory_candidate_id      TEXT    NOT NULL,
    from_state                 TEXT    NOT NULL,
    to_state                   TEXT    NOT NULL,
    resurface_trigger          TEXT,
    observed_candidate_version INTEGER NOT NULL,
    envelope_json              TEXT    NOT NULL,
    resulting_object_id        TEXT,
    recorded_at                TEXT    NOT NULL
);
CREATE INDEX idx_candidate_decision_events_candidate ON candidate_decision_events (advisory_candidate_id);

CREATE TABLE suppression_records (
    suppression_key       TEXT PRIMARY KEY,
    advisory_candidate_id TEXT NOT NULL,
    payload_json          TEXT NOT NULL,
    decided_at            TEXT NOT NULL
);
