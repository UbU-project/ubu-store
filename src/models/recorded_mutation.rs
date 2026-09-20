use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Audit row for an admitted mutation. `result_version` records the admitted
/// version, while replay reads the object's current state rather than a snapshot.
/// Zero denotes an unversioned append-only log or external-reference result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct RecordedMutation {
    pub origin_device_id: String,
    pub idempotency_key: String,
    pub envelope_json: String,
    pub canonical_payload: String,
    pub result_object_id: String,
    pub result_version: i64,
    pub recorded_at: String,
}
