use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ExternalReferenceRecord {
    pub id: String,
    pub source_type: String,
    pub source_id: String,
    pub url: Option<String>,
    pub payload_hash: Option<String>,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewExternalReferenceRecord {
    pub id: String,
    pub source_type: String,
    pub source_id: String,
    pub url: Option<String>,
    pub payload_hash: Option<String>,
    pub payload: Value,
    pub created_at: String,
}
