use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ProjectionPreviewRecord {
    pub id: String,
    pub request_id: String,
    pub status: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewProjectionPreviewRecord {
    pub id: String,
    pub request_id: String,
    pub status: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ProjectionResultRecord {
    pub id: String,
    pub preview_id: String,
    pub status: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewProjectionResultRecord {
    pub id: String,
    pub preview_id: String,
    pub status: String,
    pub payload: Value,
    pub created_at: String,
}
