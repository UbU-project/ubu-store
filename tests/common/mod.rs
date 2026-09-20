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
