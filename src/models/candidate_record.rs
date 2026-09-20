use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use ubu_core::AdvisoryCandidate;

/// Candidate-state row, deliberately distinct from canonical ObjectRecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct CandidateRecord {
    pub advisory_candidate_id: String,
    pub candidate_kind: String,
    pub lifecycle_state: String,
    pub version: i64,
    pub suppression_key: Option<String>,
    pub review_order: Option<i64>,
    pub payload_json: String,
    pub created_at: String,
    pub updated_at: String,
}

impl CandidateRecord {
    pub fn candidate(&self) -> crate::Result<AdvisoryCandidate> {
        Ok(serde_json::from_str(&self.payload_json)?)
    }
}

/// Review-only event; its envelope preserves the complete decision provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct CandidateDecisionEvent {
    pub id: String,
    pub advisory_candidate_id: String,
    pub from_state: String,
    pub to_state: String,
    pub resurface_trigger: Option<String>,
    pub observed_candidate_version: i64,
    pub envelope_json: String,
    pub resulting_object_id: Option<String>,
    pub recorded_at: String,
}
