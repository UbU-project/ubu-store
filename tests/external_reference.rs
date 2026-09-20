mod common;

use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::external_reference_record::NewExternalReferenceRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn stores_and_queries_external_reference() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    queries::store_external_reference(
        store.pool(),
        &common::append_envelope(),
        NewExternalReferenceRecord {
            id: UbuId::new(ObjectType::ExternalReference).to_string(),
            source_type: "github_issue".to_owned(),
            source_id: "42".to_owned(),
            url: Some("https://example.invalid/issues/42".to_owned()),
            payload_hash: None,
            payload: json!({"title": "issue"}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("xref stored");

    let refs = queries::query_external_references(store.pool(), Some("github_issue"))
        .await
        .expect("xrefs query");
    assert_eq!(refs.len(), 1);
}

fn reference() -> NewExternalReferenceRecord {
    NewExternalReferenceRecord {
        id: UbuId::new(ObjectType::ExternalReference).to_string(),
        source_type: "github_issue".into(),
        source_id: "42".into(),
        url: Some("https://example.invalid/issues/42".into()),
        payload_hash: None,
        payload: json!({"title": "issue"}),
        created_at: "2026-09-19T02:00:00Z".into(),
    }
}

#[tokio::test]
async fn external_reference_admits_and_replays_with_empty_preconditions() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = reference();
    let envelope = common::append_envelope();
    assert!(envelope.observed_versions.is_empty());
    let first = queries::store_external_reference(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    let audit = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(audit.result_object_id, record.id);
    assert_eq!(audit.result_version, 0);
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
    let second = queries::store_external_reference(store.pool(), &envelope, record)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        queries::query_external_references(store.pool(), None)
            .await
            .unwrap(),
        vec![first]
    );
    assert_eq!(common::ledger(store.pool()).await, vec![audit]);
    let after: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(changes, after);
}

#[tokio::test]
async fn external_reference_changed_payload_conflicts_without_changes() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = reference();
    let envelope = common::append_envelope();
    let first = queries::store_external_reference(store.pool(), &envelope, record.clone())
        .await
        .unwrap();
    let ledger = common::ledger(store.pool()).await;
    record.payload["title"] = json!("different");
    let error = queries::store_external_reference(store.pool(), &envelope, record)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ubu_store::StoreError::Core(ubu_core::UbuError::IdempotencyKeyConflict { .. })
    ));
    assert_eq!(
        queries::query_external_references(store.pool(), None)
            .await
            .unwrap(),
        vec![first]
    );
    assert_eq!(common::ledger(store.pool()).await, ledger);
}

#[tokio::test]
async fn external_reference_read_preconditions_and_ledger_failure_roll_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let record = reference();
    let mut envelope = common::append_envelope();
    envelope.observed_versions.insert(
        UbuId::new(ObjectType::Task),
        ubu_core::VersionRef::Version(1),
    );
    assert!(matches!(
        queries::store_external_reference(store.pool(), &envelope, record.clone())
            .await
            .unwrap_err(),
        ubu_store::StoreError::PreconditionFailed { .. }
    ));
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        None
    );
    envelope.observed_versions.clear();
    common::fail_ledger_inserts(store.pool()).await;
    let error = queries::store_external_reference(store.pool(), &envelope, record)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected ledger failure"));
    assert!(queries::query_external_references(store.pool(), None)
        .await
        .unwrap()
        .is_empty());
    assert!(common::ledger(store.pool()).await.is_empty());
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn external_reference_invalid_envelope_or_record_rolls_back() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut envelope = common::append_envelope();
    envelope.actor_identity_id = UbuId::new(ObjectType::Task);
    assert!(matches!(
        queries::store_external_reference(store.pool(), &envelope, reference())
            .await
            .unwrap_err(),
        ubu_store::StoreError::Core(ubu_core::UbuError::WrongIdObjectType { .. })
    ));
    let envelope = common::append_envelope();
    let mut invalid = reference();
    invalid.created_at = "not-a-time".into();
    assert!(
        queries::store_external_reference(store.pool(), &envelope, invalid)
            .await
            .is_err()
    );
    assert!(queries::query_external_references(store.pool(), None)
        .await
        .unwrap()
        .is_empty());
    assert!(common::ledger(store.pool()).await.is_empty());
}
