// Each integration-test crate uses its own subset of these shared helpers.
#![allow(dead_code)]

use sqlx::SqlitePool;
use ubu_core::{
    AuthoritySource, CausalityIssuer, DeviceId, EnvelopeRequest, LocalIssuer, MutationEnvelope,
    ObjectType, UbuId, UbuTimestamp, VersionRef,
};
use ubu_store::models::object_record::{NewObjectRecord, ObjectRecord};

pub fn envelope_for(id: &str, expected: VersionRef) -> MutationEnvelope {
    // Invalid-id regression tests must reach the store's own rejection path.
    // An invalid target cannot be represented in the typed precondition map.
    let observed_versions = UbuId::parse(id)
        .into_iter()
        .map(|id| (id, expected))
        .collect();
    issue_envelope(observed_versions)
}

pub fn append_envelope() -> MutationEnvelope {
    issue_envelope(Default::default())
}

fn issue_envelope(
    observed_versions: std::collections::BTreeMap<UbuId, VersionRef>,
) -> MutationEnvelope {
    let now = UbuTimestamp::parse("2026-09-19T09:00:00Z").unwrap();
    LocalIssuer::with_clock(DeviceId::parse("test-device").unwrap(), move || now)
        .issue(EnvelopeRequest {
            observed_versions,
            actor_identity_id: UbuId::new(ObjectType::Identity),
            authority_source: AuthoritySource::User,
            effective_time: now,
            observed_policy_versions: None,
            execution_context: None,
        })
        .unwrap()
}

/// Test-only adapter preserving the existing admission fixtures and assertions.
pub async fn admit_object(
    pool: &SqlitePool,
    record: NewObjectRecord,
) -> ubu_store::Result<ObjectRecord> {
    let envelope = envelope_for(&record.id, VersionRef::Absent);
    ubu_store::queries::admit_object(pool, &envelope, record).await
}

pub async fn ledger(
    pool: &SqlitePool,
) -> Vec<ubu_store::models::recorded_mutation::RecordedMutation> {
    sqlx::query_as("SELECT * FROM mutation_envelopes ORDER BY origin_device_id, idempotency_key")
        .fetch_all(pool)
        .await
        .unwrap()
}

pub async fn fail_ledger_inserts(pool: &SqlitePool) {
    sqlx::query("CREATE TRIGGER reject_envelope BEFORE INSERT ON mutation_envelopes BEGIN SELECT RAISE(ABORT, 'injected ledger failure'); END")
        .execute(pool).await.unwrap();
}
