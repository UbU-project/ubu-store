use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ObjectRecord {
    pub id: String,
    pub object_type: String,
    pub version: i64,
    pub status: String,
    pub compartment_label: String,
    pub payload_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewObjectRecord {
    pub id: String,
    pub object_type: String,
    pub version: i64,
    pub status: String,
    pub compartment_label: String,
    pub payload: Value,
    pub created_at: String,
    pub updated_at: String,
}
