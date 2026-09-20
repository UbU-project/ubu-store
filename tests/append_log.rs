mod common;

use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::log_record::NewLogRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn appends_log_entry() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let log_id = UbuId::new(ObjectType::LogEntry).to_string();
    let object_id = UbuId::new(ObjectType::Task).to_string();
    let log = queries::append_log_entry(
        store.pool(),
        &common::append_envelope(),
        NewLogRecord {
            id: log_id.clone(),
            event_type: "object_admitted".to_owned(),
            object_refs: json!([object_id]),
            payload: json!({"ok": true}),
            provenance: json!({
                "created_at": "2026-06-10T14:30:00Z",
                "authority_source": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("log appended");

    assert_eq!(log.id, log_id);
}

fn log_record() -> NewLogRecord {
    NewLogRecord {
        id: UbuId::new(ObjectType::LogEntry).to_string(),
        event_type: "fact_observed".into(),
        object_refs: json!([]),
        payload: json!({"fact": "observed"}),
        provenance: json!({"created_at": "2026-09-19T02:00:00Z", "authority_source": "user"}),
        created_at: "2026-09-19T02:00:00Z".into(),
    }
}

async fn logs(store: &UbuStore) -> Vec<ubu_store::models::log_record::LogRecord> {
    sqlx::query_as("SELECT * FROM logs ORDER BY id")
        .fetch_all(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn log_empty_preconditions_admit_and_replay_without_writes() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = log_record();
    let envelope = common::append_envelope();
    assert!(envelope.observed_versions.is_empty());
    let first = queries::append_log_entry(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    let audit = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(audit.result_object_id, record.id);
    assert_eq!(audit.result_version, 0);
    assert_eq!(audit.recorded_at, envelope.recorded_time.to_string());
    assert_eq!(
        audit.canonical_payload.as_bytes(),
        ubu_core::canonical_payload_bytes(&record.payload)
    );
    assert_eq!(
        serde_json::from_str::<ubu_core::MutationEnvelope>(&audit.envelope_json).unwrap(),
        envelope
    );
    let changes: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let second = queries::append_log_entry(store.pool(), &envelope, record)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(logs(&store).await, vec![first]);
    assert_eq!(common::ledger(store.pool()).await, vec![audit]);
    let after: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(changes, after);
}

#[tokio::test]
async fn log_changed_payload_conflicts_without_changes() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = log_record();
    let envelope = common::append_envelope();
    let first = queries::append_log_entry(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    let before = common::ledger(store.pool()).await;
    record.payload["fact"] = json!("different");
    let error = queries::append_log_entry(store.pool(), &envelope, record)
        .await
        .unwrap_err();
    assert!(
        matches!(error, ubu_store::StoreError::Core(ubu_core::UbuError::IdempotencyKeyConflict { origin_device_id, idempotency_key })
        if origin_device_id == envelope.origin_device_id.as_str() && idempotency_key == envelope.idempotency_key.as_str())
    );
    assert_eq!(logs(&store).await, vec![first]);
    assert_eq!(common::ledger(store.pool()).await, before);
}

#[tokio::test]
async fn log_unsatisfied_read_precondition_blocks_append() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = log_record();
    let mut envelope = common::append_envelope();
    let dependency = UbuId::new(ObjectType::Task);
    envelope
        .observed_versions
        .insert(dependency.clone(), ubu_core::VersionRef::Version(1));
    let error = queries::append_log_entry(store.pool(), &envelope, record.clone())
        .await
        .unwrap_err();
    assert!(
        matches!(error, ubu_store::StoreError::PreconditionFailed { object_id, expected, actual }
        if object_id == dependency.as_str() && expected == "v1" && actual == "absent")
    );
    assert!(logs(&store).await.is_empty());
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        None
    );
    envelope
        .observed_versions
        .insert(dependency, ubu_core::VersionRef::Absent);
    queries::append_log_entry(store.pool(), &envelope, record)
        .await
        .unwrap();
    assert_eq!(logs(&store).await.len(), 1);
}

#[tokio::test]
async fn log_ledger_failure_rolls_back_insert() {
    let store = UbuStore::in_memory().await.unwrap();
    common::fail_ledger_inserts(store.pool()).await;
    let envelope = common::append_envelope();
    let error = queries::append_log_entry(store.pool(), &envelope, log_record())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected ledger failure"));
    assert!(logs(&store).await.is_empty());
    assert!(common::ledger(store.pool()).await.is_empty());
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn log_invalid_envelope_or_record_rolls_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut envelope = common::append_envelope();
    envelope.actor_identity_id = UbuId::new(ObjectType::Task);
    assert!(matches!(
        queries::append_log_entry(store.pool(), &envelope, log_record())
            .await
            .unwrap_err(),
        ubu_store::StoreError::Core(ubu_core::UbuError::WrongIdObjectType { .. })
    ));
    let envelope = common::append_envelope();
    let mut invalid = log_record();
    invalid.provenance = json!({});
    assert!(queries::append_log_entry(store.pool(), &envelope, invalid)
        .await
        .is_err());
    assert!(logs(&store).await.is_empty());
    assert!(common::ledger(store.pool()).await.is_empty());
}

#[tokio::test]
async fn cross_writer_key_is_global_per_device() {
    let store = UbuStore::in_memory().await.unwrap();
    // A LogEntry canonical object deliberately uses the same id kind as a logs
    // row, testing that table ownership cannot be inferred from the prefix alone.
    let id = UbuId::new(ObjectType::LogEntry);
    let payload = json!({"id": id.as_str(), "authority_source": "user", "fact": "object"});
    let object = ubu_store::models::object_record::NewObjectRecord {
        id: id.to_string(),
        object_type: "LogEntry".into(),
        version: 1,
        status: "active".into(),
        compartment_label: "default".into(),
        payload: payload.clone(),
        created_at: "2026-09-19T02:00:00Z".into(),
        updated_at: "2026-09-19T02:00:00Z".into(),
    };
    let envelope = common::envelope_for(id.as_str(), ubu_core::VersionRef::Absent);
    let admitted = queries::admit_object(store.pool(), &envelope, object)
        .await
        .unwrap();
    let ledger = common::ledger(store.pool()).await;
    let mut log = log_record();
    log.id = id.to_string();
    let error = queries::append_log_entry(store.pool(), &envelope, log.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ubu_store::StoreError::Core(ubu_core::UbuError::IdempotencyKeyConflict { .. })
    ));
    log.payload = payload;
    let error = queries::append_log_entry(store.pool(), &envelope, log)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ubu_store::StoreError::ReplayTargetMismatch {
            expected_table: "logs",
            ..
        }
    ));
    assert!(logs(&store).await.is_empty());
    assert_eq!(common::ledger(store.pool()).await, ledger);
    assert_eq!(
        queries::get_current_state(store.pool(), id.as_str())
            .await
            .unwrap(),
        Some(admitted)
    );
}
