use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct WorkerSubmissionRecord {
    pub id: String,
    pub candidate_id: String,
    pub object_type: String,
    pub status: String,
    pub payload_json: String,
    pub authority_source: String,
    pub submitted_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewWorkerSubmissionRecord {
    pub id: String,
    pub candidate_id: String,
    pub object_type: String,
    pub status: String,
    pub payload: Value,
    pub authority_source: String,
    pub submitted_at: String,
    pub created_at: String,
}
