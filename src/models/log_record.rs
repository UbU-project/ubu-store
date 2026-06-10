use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct LogRecord {
    pub id: String,
    pub event_type: String,
    pub object_refs_json: String,
    pub payload_json: String,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewLogRecord {
    pub id: String,
    pub event_type: String,
    pub object_refs: Value,
    pub payload: Value,
    pub provenance: Value,
    pub created_at: String,
}
