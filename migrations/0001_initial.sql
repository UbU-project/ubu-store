CREATE TABLE objects (
    id TEXT PRIMARY KEY,
    object_type TEXT NOT NULL,
    version INTEGER NOT NULL,
    status TEXT NOT NULL,
    compartment_label TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE logs (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    object_refs_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    provenance_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE external_references (
    id TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    url TEXT,
    payload_hash TEXT,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE plans (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE calendars (
    id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL,
    window_start TEXT NOT NULL,
    window_end TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE snapshots (
    id TEXT PRIMARY KEY,
    object_refs_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE worker_submissions (
    id TEXT PRIMARY KEY,
    candidate_id TEXT NOT NULL,
    object_type TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    authority_source TEXT NOT NULL,
    submitted_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE projection_previews (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE projection_results (
    id TEXT PRIMARY KEY,
    preview_id TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_objects_type_status ON objects (object_type, status);
CREATE INDEX idx_objects_compartment ON objects (compartment_label);
CREATE INDEX idx_logs_created_at ON logs (created_at);
CREATE INDEX idx_external_references_source ON external_references (source_type, source_id);
CREATE INDEX idx_calendars_plan_id ON calendars (plan_id);
CREATE INDEX idx_projection_results_preview_id ON projection_results (preview_id);
