mod common;

use serde_json::{json, Value};
use ubu_core::store::CandidateObject;
use ubu_core::{
    canonical_payload_bytes, AuthoritySource, DeviceId, MutationEnvelope, ObjectType, UbuError,
    UbuId, UbuTimestamp, VersionRef,
};
use ubu_store::models::object_record::{NewObjectRecord, ObjectRecord};
use ubu_store::{queries, StoreError, UbuStore};

fn task() -> NewObjectRecord {
    let id = UbuId::new(ObjectType::Task).to_string();
    NewObjectRecord {
        id: id.clone(),
        object_type: "Task".into(),
        version: 1,
        status: "active".into(),
        compartment_label: "default".into(),
        payload: json!({
            "id": id, "title": "Original", "status": "active",
            "provenance": {"created_at": "2026-09-19T02:00:00Z", "authority_source": "user"}
        }),
        created_at: "2026-09-19T02:00:00Z".into(),
        updated_at: "2026-09-19T09:00:00Z".into(),
    }
}

async fn envelope_count(store: &UbuStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM mutation_envelopes")
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn assert_rolled_back(
    store: &UbuStore,
    envelope: &MutationEnvelope,
    id: &str,
    before: Option<&ObjectRecord>,
    count: i64,
) {
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        queries::get_current_state(store.pool(), id)
            .await
            .unwrap()
            .as_ref(),
        before
    );
    assert_eq!(envelope_count(store).await, count);
}

fn assert_precondition(error: StoreError, id: &str, expected_value: &str, actual_value: &str) {
    match error {
        StoreError::PreconditionFailed {
            object_id,
            expected,
            actual,
        } => {
            assert_eq!(object_id, id);
            assert_eq!(expected, expected_value);
            assert_eq!(actual, actual_value);
        }
        other => panic!("expected precondition failure, got {other:?}"),
    }
}

#[tokio::test]
async fn create_sets_version_one_and_records_envelope() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = task();
    record.version = 17; // The target precondition owns the admitted version.
    let mut envelope = common::envelope_for(&record.id, VersionRef::Absent);
    envelope.created_time = UbuTimestamp::parse("2026-09-19T02:00:00Z").unwrap();
    envelope.effective_time = envelope.created_time;
    let admitted = queries::admit_object(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    assert_eq!(admitted.version, 1);
    assert_eq!(admitted.id, record.id);
    assert_eq!(admitted.created_at, record.created_at);
    let audit = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(audit.origin_device_id, envelope.origin_device_id.as_str());
    assert_eq!(audit.idempotency_key, envelope.idempotency_key.as_str());
    assert_eq!(audit.result_object_id, record.id);
    assert_eq!(audit.result_version, 1);
    assert_eq!(audit.recorded_at, envelope.recorded_time.to_string());
    assert_eq!(
        serde_json::from_str::<MutationEnvelope>(&audit.envelope_json).unwrap(),
        envelope
    );
    assert_eq!(
        audit.canonical_payload.as_bytes(),
        canonical_payload_bytes(&record.payload)
    );
    assert_eq!(envelope_count(&store).await, 1);
    let index_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_mutation_envelopes_object'")
        .fetch_one(store.pool()).await.unwrap();
    assert_eq!(index_count, 1);
}

#[tokio::test]
async fn create_collision_rolls_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    let before = common::admit_object(store.pool(), record.clone())
        .await
        .unwrap();
    let envelope = common::envelope_for(&record.id, VersionRef::Absent);
    let error = queries::admit_object(store.pool(), &envelope, record.clone())
        .await
        .unwrap_err();
    assert_precondition(error, &record.id, "absent", "v1");
    assert_rolled_back(&store, &envelope, &record.id, Some(&before), 1).await;
}

#[tokio::test]
async fn update_increments_version_and_preserves_created_at() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = task();
    let before = common::admit_object(store.pool(), record.clone())
        .await
        .unwrap();
    let envelope = common::envelope_for(&record.id, VersionRef::Version(1));
    record.version = 99;
    record.created_at = "2026-09-19T12:00:00Z".into();
    record.updated_at = "2026-09-19T12:30:00Z".into();
    record.status = "completed".into();
    record.compartment_label = "work".into();
    record.payload["status"] = json!("completed");
    record.payload["title"] = json!("Updated");
    let after = queries::admit_object(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    assert_eq!(after.version, 2);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, record.updated_at);
    assert_eq!(after.status, record.status);
    assert_eq!(after.compartment_label, record.compartment_label);
    assert_eq!(
        serde_json::from_str::<Value>(&after.payload_json).unwrap(),
        record.payload
    );
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap()
            .unwrap()
            .result_version,
        2
    );
    assert_eq!(envelope_count(&store).await, 2);
}

#[tokio::test]
async fn stale_update_rolls_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = task();
    common::admit_object(store.pool(), record.clone())
        .await
        .unwrap();
    let update = common::envelope_for(&record.id, VersionRef::Version(1));
    record.payload["title"] = json!("Updated");
    let before = queries::admit_object(store.pool(), &update, record.clone())
        .await
        .unwrap();
    let stale = common::envelope_for(&record.id, VersionRef::Version(1));
    record.payload["title"] = json!("Stale overwrite");
    let error = queries::admit_object(store.pool(), &stale, record.clone())
        .await
        .unwrap_err();
    assert_precondition(error, &record.id, "v1", "v2");
    assert_rolled_back(&store, &stale, &record.id, Some(&before), 2).await;
}

#[tokio::test]
async fn missing_target_precondition_rolls_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    let mut envelope = common::envelope_for(&record.id, VersionRef::Absent);
    envelope.observed_versions.clear();
    let error = queries::admit_object(store.pool(), &envelope, record.clone())
        .await
        .unwrap_err();
    assert!(
        matches!(error, StoreError::MissingTargetPrecondition { object_id } if object_id == record.id)
    );
    assert_rolled_back(&store, &envelope, &record.id, None, 0).await;
}

#[tokio::test]
async fn non_target_preconditions_block_otherwise_valid_target() {
    let store = UbuStore::in_memory().await.unwrap();
    let dependency = task();
    let dependency_before = common::admit_object(store.pool(), dependency.clone())
        .await
        .unwrap();
    let target = task();
    for expected in [VersionRef::Version(2), VersionRef::Absent] {
        let mut envelope = common::envelope_for(&target.id, VersionRef::Absent);
        envelope
            .observed_versions
            .insert(UbuId::parse(&dependency.id).unwrap(), expected);
        let error = queries::admit_object(store.pool(), &envelope, target.clone())
            .await
            .unwrap_err();
        assert_precondition(
            error,
            &dependency.id,
            if expected == VersionRef::Absent {
                "absent"
            } else {
                "v2"
            },
            "v1",
        );
        assert_rolled_back(&store, &envelope, &target.id, None, 1).await;
        assert_eq!(
            queries::get_current_state(store.pool(), &dependency.id)
                .await
                .unwrap(),
            Some(dependency_before.clone())
        );
    }
    let mut envelope = common::envelope_for(&target.id, VersionRef::Absent);
    envelope.observed_versions.insert(
        UbuId::parse(&dependency.id).unwrap(),
        VersionRef::Version(1),
    );
    envelope
        .observed_versions
        .insert(UbuId::new(ObjectType::Task), VersionRef::Absent);
    queries::admit_object(store.pool(), &envelope, target)
        .await
        .unwrap();
}

#[tokio::test]
async fn observed_version_requires_an_existing_object_without_integer_truncation() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    for version in [0, 1, u64::MAX] {
        let envelope = common::envelope_for(&record.id, VersionRef::Version(version));
        let error = queries::admit_object(store.pool(), &envelope, record.clone())
            .await
            .unwrap_err();
        assert_precondition(error, &record.id, &format!("v{version}"), "absent");
        assert_rolled_back(&store, &envelope, &record.id, None, 0).await;
    }
}

#[tokio::test]
async fn replay_same_payload_performs_no_write() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    let envelope = common::envelope_for(&record.id, VersionRef::Absent);
    let first = queries::admit_object(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    let audit = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap();
    let changes: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let second = queries::admit_object(store.pool(), &envelope, record)
        .await
        .unwrap();
    let changes_after: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.version, 1);
    assert_eq!(changes, changes_after);
    assert_eq!(envelope_count(&store).await, 1);
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        audit
    );
}

#[tokio::test]
async fn replay_different_payload_is_conflict_without_changes() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = task();
    let envelope = common::envelope_for(&record.id, VersionRef::Absent);
    let before = queries::admit_object(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    let audit_before = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap();
    record.payload["title"] = json!("Conflicting payload");
    let error = queries::admit_object(store.pool(), &envelope, record)
        .await
        .unwrap_err();
    assert!(
        matches!(error, StoreError::Core(UbuError::IdempotencyKeyConflict { origin_device_id, idempotency_key })
        if origin_device_id == envelope.origin_device_id.as_str() && idempotency_key == envelope.idempotency_key.as_str())
    );
    assert_eq!(
        queries::get_current_state(store.pool(), &before.id)
            .await
            .unwrap(),
        Some(before)
    );
    // A conflict preserves the pre-existing audit row, rather than returning None.
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        audit_before
    );
    assert_eq!(envelope_count(&store).await, 1);
}

#[tokio::test]
async fn same_key_on_different_devices_is_independent() {
    let store = UbuStore::in_memory().await.unwrap();
    let first = task();
    let second = task();
    let a = common::envelope_for(&first.id, VersionRef::Absent);
    let mut b = common::envelope_for(&second.id, VersionRef::Absent);
    b.idempotency_key = a.idempotency_key.clone();
    b.origin_device_id = DeviceId::parse("another-device").unwrap();
    queries::admit_object(store.pool(), &a, first)
        .await
        .unwrap();
    queries::admit_object(store.pool(), &b, second)
        .await
        .unwrap();
    for envelope in [&a, &b] {
        assert!(
            queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
                .await
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(envelope_count(&store).await, 2);
}

#[tokio::test]
async fn replay_returns_recorded_targets_current_state() {
    let store = UbuStore::in_memory().await.unwrap();
    let original = task();
    let envelope = common::envelope_for(&original.id, VersionRef::Absent);
    queries::admit_object(store.pool(), &envelope, original.clone())
        .await
        .unwrap();
    let mut update = original.clone();
    update.payload["title"] = json!("Version two");
    let update_envelope = common::envelope_for(&original.id, VersionRef::Version(1));
    let latest = queries::admit_object(store.pool(), &update_envelope, update)
        .await
        .unwrap();
    let mut replay = original;
    // Per ticket, metadata outside payload is not part of replay comparison.
    replay.id = UbuId::new(ObjectType::Task).to_string();
    replay.version = 0;
    assert_eq!(
        queries::admit_object(store.pool(), &envelope, replay)
            .await
            .unwrap(),
        latest
    );
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap()
            .unwrap()
            .result_version,
        1
    );
    assert_eq!(envelope_count(&store).await, 2);
}

#[tokio::test]
async fn invalid_envelope_and_record_validation_roll_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    let mut envelope = common::envelope_for(&record.id, VersionRef::Absent);
    envelope.actor_identity_id = UbuId::new(ObjectType::Task);
    assert!(matches!(
        queries::admit_object(store.pool(), &envelope, record.clone())
            .await
            .unwrap_err(),
        StoreError::Core(UbuError::WrongIdObjectType { .. })
    ));
    assert_rolled_back(&store, &envelope, &record.id, None, 0).await;
    let envelope = common::envelope_for(&record.id, VersionRef::Absent);
    let mut invalid = record.clone();
    invalid
        .payload
        .as_object_mut()
        .unwrap()
        .remove("provenance");
    assert!(queries::admit_object(store.pool(), &envelope, invalid)
        .await
        .is_err());
    assert_rolled_back(&store, &envelope, &record.id, None, 0).await;
}

#[tokio::test]
async fn envelope_insert_failure_rolls_back_object_insert_and_update() {
    let store = UbuStore::in_memory().await.unwrap();
    let existing = task();
    let before = common::admit_object(store.pool(), existing.clone())
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_envelope BEFORE INSERT ON mutation_envelopes BEGIN SELECT RAISE(ABORT, 'injected ledger failure'); END")
        .execute(store.pool()).await.unwrap();
    for (mut record, expected, previous) in [
        (task(), VersionRef::Absent, None),
        (existing, VersionRef::Version(1), Some(&before)),
    ] {
        record.payload["title"] = json!("Must roll back");
        let envelope = common::envelope_for(&record.id, expected);
        let error = queries::admit_object(store.pool(), &envelope, record.clone())
            .await
            .unwrap_err();
        assert!(matches!(error, StoreError::Sqlx(_)));
        assert!(error.to_string().contains("injected ledger failure"));
        assert_rolled_back(&store, &envelope, &record.id, previous, 1).await;
    }
}

#[tokio::test]
async fn sqlite_version_exhaustion_rolls_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    common::admit_object(store.pool(), record.clone())
        .await
        .unwrap();
    sqlx::query("UPDATE objects SET version = ? WHERE id = ?")
        .bind(i64::MAX)
        .bind(&record.id)
        .execute(store.pool())
        .await
        .unwrap();
    let before = queries::get_current_state(store.pool(), &record.id)
        .await
        .unwrap()
        .unwrap();
    let envelope = common::envelope_for(&record.id, VersionRef::Version(i64::MAX as u64));
    assert!(matches!(
        queries::admit_object(store.pool(), &envelope, record.clone())
            .await
            .unwrap_err(),
        StoreError::ObjectVersionExhausted { .. }
    ));
    assert_rolled_back(&store, &envelope, &record.id, Some(&before), 1).await;
}

#[tokio::test]
async fn candidate_still_writes_active_canonical_row_without_envelope() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    let candidate = CandidateObject {
        candidate_id: record.id.clone(),
        object_type: record.object_type,
        payload: record.payload.clone(),
        submitted_at: UbuTimestamp::parse(&record.created_at).unwrap(),
        authority_source: AuthoritySource::User,
    };
    let admitted = queries::admit_candidate_object(store.pool(), candidate, "default")
        .await
        .unwrap();
    assert_eq!(admitted.version, 1);
    assert_eq!(admitted.status, "active");
    assert_eq!(
        serde_json::from_str::<Value>(&admitted.payload_json).unwrap(),
        record.payload
    );
    assert_eq!(
        queries::get_current_state(store.pool(), &record.id)
            .await
            .unwrap(),
        Some(admitted.clone())
    );
    assert!(queries::query_active_tasks(store.pool())
        .await
        .unwrap()
        .contains(&admitted));
    assert_eq!(envelope_count(&store).await, 0);
}
