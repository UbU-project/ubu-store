mod common;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use ubu_core::core::{JsonScalar, UniverseState};
use ubu_core::id_registry::ObjectType;
use ubu_core::{AuthoritySource, UbuId, UbuTimestamp};
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn admits_universe_state_and_round_trips_all_collections() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let payload = universe_state_payload(&state);

    let admitted = common::admit_object(
        store.pool(),
        NewObjectRecord {
            id: state.id.to_string(),
            object_type: "UniverseState".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: payload.clone(),
            created_at: "2026-06-22T13:00:00Z".to_owned(),
            updated_at: "2026-06-22T13:00:00Z".to_owned(),
        },
    )
    .await
    .expect("universe state admitted");

    let stored_payload: Value =
        serde_json::from_str(&admitted.payload_json).expect("stored payload is json");
    assert_eq!(stored_payload, payload);

    let round_tripped_state: UniverseState =
        serde_json::from_value(stored_payload).expect("stored payload is a UniverseState");
    assert_eq!(round_tripped_state.id, state.id);
    assert_eq!(round_tripped_state.captured_at, state.captured_at);
    assert_eq!(round_tripped_state.facts, state.facts);
    assert_eq!(round_tripped_state.numeric_values, state.numeric_values);
    assert_eq!(round_tripped_state.set_memberships, state.set_memberships);
    assert_eq!(round_tripped_state.event_markers, state.event_markers);
    assert_eq!(round_tripped_state.source_summary, state.source_summary);
    assert_eq!(
        round_tripped_state.confidence_summary,
        state.confidence_summary
    );
}

#[tokio::test]
async fn admits_universe_state_with_envelope_and_provenance_authority_source() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let payload = universe_state_payload(&state);

    let admitted = common::admit_object(
        store.pool(),
        NewObjectRecord {
            id: state.id.to_string(),
            object_type: ObjectType::UniverseState.as_str().to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: payload.clone(),
            created_at: "2026-06-22T13:00:00Z".to_owned(),
            updated_at: "2026-06-22T13:00:00Z".to_owned(),
        },
    )
    .await
    .expect("canonical state admitted with envelope");

    let stored_payload: Value =
        serde_json::from_str(&admitted.payload_json).expect("stored payload is json");
    assert_eq!(stored_payload["provenance"]["authority_source"], "user");
    assert_eq!(stored_payload, payload);
}

#[tokio::test]
async fn rejects_universe_state_without_provenance_authority_source() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let mut payload = universe_state_payload(&state);
    payload["provenance"] = json!({
        "created_at": "2026-06-22T13:00:00Z"
    });

    let result = common::admit_object(
        store.pool(),
        NewObjectRecord {
            id: state.id.to_string(),
            object_type: "UniverseState".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload,
            created_at: "2026-06-22T13:00:00Z".to_owned(),
            updated_at: "2026-06-22T13:00:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rejects_universe_state_with_non_ustate_id_prefix() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let task_id = UbuId::new(ObjectType::Task).to_string();
    let mut payload = universe_state_payload(&state);
    payload["id"] = json!(task_id);

    let result = common::admit_object(
        store.pool(),
        NewObjectRecord {
            id: task_id,
            object_type: "UniverseState".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload,
            created_at: "2026-06-22T13:00:00Z".to_owned(),
            updated_at: "2026-06-22T13:00:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn persists_updated_universe_state_as_new_current_version() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let payload = universe_state_payload(&state);

    common::admit_object(
        store.pool(),
        NewObjectRecord {
            id: state.id.to_string(),
            object_type: "UniverseState".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload,
            created_at: "2026-06-22T13:00:00Z".to_owned(),
            updated_at: "2026-06-22T13:00:00Z".to_owned(),
        },
    )
    .await
    .expect("universe state admitted");

    // Simulate a container whose effects were applied elsewhere: mutate every
    // one of the four collections plus a shell field.
    let mut updated = state.clone();
    updated
        .facts
        .insert("task.status".to_owned(), json!("complete"));
    updated.numeric_values.insert("energy".to_owned(), 0.25);
    updated.set_memberships.insert(
        "focus.tags".to_owned(),
        BTreeSet::from([JsonScalar::String("review".to_owned())]),
    );
    updated.event_markers.insert(
        "task.completed".to_owned(),
        vec![serde_json::Map::from_iter([(
            "at".to_owned(),
            json!("2026-06-22T13:30:00Z"),
        )])],
    );
    updated.confidence_summary = Some("updated confidence summary".to_owned());

    let persisted = queries::persist_universe_state(
        store.pool(),
        &common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1)),
        &updated,
        AuthoritySource::System,
    )
    .await
    .expect("universe state persisted");
    assert_eq!(persisted.version, 2);

    let current = queries::get_current_state(store.pool(), &state.id.to_string())
        .await
        .expect("current state readable")
        .expect("current state present");
    assert_eq!(current.version, 2);

    let stored_payload: Value =
        serde_json::from_str(&current.payload_json).expect("stored payload is json");
    assert_eq!(stored_payload["provenance"]["authority_source"], "system");
    // schema_version shell metadata is preserved across the version bump.
    assert_eq!(
        stored_payload["schema_version"],
        json!("core/universe-state/0.1")
    );

    let round_tripped_state: UniverseState =
        serde_json::from_value(stored_payload).expect("stored payload is a UniverseState");
    assert_eq!(round_tripped_state.id, updated.id);
    assert_eq!(round_tripped_state.captured_at, updated.captured_at);
    assert_eq!(round_tripped_state.facts, updated.facts);
    assert_eq!(round_tripped_state.numeric_values, updated.numeric_values);
    assert_eq!(round_tripped_state.set_memberships, updated.set_memberships);
    assert_eq!(round_tripped_state.event_markers, updated.event_markers);
    assert_eq!(round_tripped_state.source_summary, updated.source_summary);
    assert_eq!(
        round_tripped_state.confidence_summary,
        updated.confidence_summary
    );
}

#[tokio::test]
async fn persist_universe_state_requires_existing_current_version() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();

    let result = queries::persist_universe_state(
        store.pool(),
        &common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1)),
        &state,
        AuthoritySource::System,
    )
    .await;

    assert!(result.is_err());
}

fn populated_universe_state() -> UniverseState {
    let mut state = UniverseState::new(
        UbuTimestamp::parse("2026-06-22T12:45:00Z").expect("valid captured_at"),
        "admitted from store ST6 test fixture",
    );

    state.facts = BTreeMap::from([
        ("task.status".to_owned(), json!("active")),
        (
            "calendar.window".to_owned(),
            json!({
                "start": "2026-06-22T13:00:00Z",
                "end": "2026-06-22T14:00:00Z"
            }),
        ),
    ]);
    state.numeric_values = BTreeMap::from([
        ("energy".to_owned(), 0.75),
        ("available_minutes".to_owned(), 45.0),
    ]);
    state.set_memberships = BTreeMap::from([(
        "focus.tags".to_owned(),
        BTreeSet::from([
            JsonScalar::String("deep-work".to_owned()),
            JsonScalar::String("calendar".to_owned()),
        ]),
    )]);
    state.event_markers = BTreeMap::from([(
        "calendar.accepted".to_owned(),
        vec![serde_json::Map::from_iter([
            ("at".to_owned(), json!("2026-06-22T12:50:00Z")),
            ("source".to_owned(), json!("user")),
        ])],
    )]);
    state.confidence_summary = Some("fixture confidence summary".to_owned());

    state
}

fn universe_state_payload(state: &UniverseState) -> Value {
    let mut payload = serde_json::to_value(state).expect("UniverseState serializes");
    payload["schema_version"] = json!("core/universe-state/0.1");
    payload["provenance"] = json!({
        "created_at": "2026-06-22T13:00:00Z",
        "created_by": "store-st6-test",
        "authority_source": "user"
    });
    payload
}

async fn seed_universe(
    store: &UbuStore,
    state: &UniverseState,
) -> ubu_store::models::object_record::ObjectRecord {
    common::admit_object(
        store.pool(),
        NewObjectRecord {
            id: state.id.to_string(),
            object_type: "UniverseState".into(),
            version: 1,
            status: "active".into(),
            compartment_label: "default".into(),
            payload: universe_state_payload(state),
            created_at: "2026-06-22T13:00:00Z".into(),
            updated_at: "2026-06-22T13:00:00Z".into(),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn universe_envelope_update_replay_and_stale_precondition() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut state = populated_universe_state();
    let first = seed_universe(&store, &state).await;
    state.numeric_values.insert("energy".into(), 0.5);
    let mut envelope = common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1));
    envelope.created_time = UbuTimestamp::parse("2026-09-19T02:00:00Z").unwrap();
    let updated =
        queries::persist_universe_state(store.pool(), &envelope, &state, AuthoritySource::System)
            .await
            .unwrap();
    assert_eq!(updated.version, 2);
    assert_eq!(updated.created_at, first.created_at);
    assert_eq!(updated.updated_at, envelope.recorded_time.to_string());
    let payload: Value = serde_json::from_str(&updated.payload_json).unwrap();
    assert_eq!(
        payload["provenance"]["created_at"],
        envelope.created_time.to_string()
    );
    assert_eq!(payload["schema_version"], "core/universe-state/0.1");
    let audit = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(audit.result_version, 2);
    assert_eq!(
        serde_json::from_str::<ubu_core::MutationEnvelope>(&audit.envelope_json).unwrap(),
        envelope
    );
    let before = common::ledger(store.pool()).await;
    let changes: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let replay =
        queries::persist_universe_state(store.pool(), &envelope, &state, AuthoritySource::System)
            .await
            .unwrap();
    assert_eq!(replay, updated);
    assert_eq!(common::ledger(store.pool()).await, before);
    let after_changes: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(changes, after_changes);
    let stale = common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1));
    let error =
        queries::persist_universe_state(store.pool(), &stale, &state, AuthoritySource::System)
            .await
            .unwrap_err();
    assert!(
        matches!(error, ubu_store::StoreError::PreconditionFailed { object_id, expected, actual }
        if object_id == state.id.as_str() && expected == "v1" && actual == "v2")
    );
    assert_eq!(
        queries::get_current_state(store.pool(), state.id.as_str())
            .await
            .unwrap(),
        Some(updated)
    );
    assert_eq!(common::ledger(store.pool()).await, before);
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &stale.mutation_key())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn universe_payload_or_authority_change_conflicts_on_replay() {
    let store = UbuStore::in_memory().await.unwrap();
    let state = populated_universe_state();
    seed_universe(&store, &state).await;
    let envelope = common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1));
    let updated =
        queries::persist_universe_state(store.pool(), &envelope, &state, AuthoritySource::System)
            .await
            .unwrap();
    let ledger = common::ledger(store.pool()).await;
    let mut changed = state.clone();
    changed.numeric_values.insert("energy".into(), 0.1);
    for (payload, authority) in [
        (&changed, AuthoritySource::System),
        (&state, AuthoritySource::User),
    ] {
        let error = queries::persist_universe_state(store.pool(), &envelope, payload, authority)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ubu_store::StoreError::Core(ubu_core::UbuError::IdempotencyKeyConflict { .. })
        ));
        assert_eq!(
            queries::get_current_state(store.pool(), state.id.as_str())
                .await
                .unwrap(),
            Some(updated.clone())
        );
        assert_eq!(common::ledger(store.pool()).await, ledger);
    }
}

#[tokio::test]
async fn universe_requires_version_target_and_checks_read_preconditions() {
    let store = UbuStore::in_memory().await.unwrap();
    let state = populated_universe_state();
    let missing = common::append_envelope();
    assert!(matches!(
        queries::persist_universe_state(store.pool(), &missing, &state, AuthoritySource::System)
            .await
            .unwrap_err(),
        ubu_store::StoreError::MissingTargetPrecondition { .. }
    ));
    let absent = common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Absent);
    assert!(matches!(
        queries::persist_universe_state(store.pool(), &absent, &state, AuthoritySource::System)
            .await
            .unwrap_err(),
        ubu_store::StoreError::PreconditionFailed { .. }
    ));
    assert_eq!(
        queries::get_current_state(store.pool(), state.id.as_str())
            .await
            .unwrap(),
        None
    );
    assert!(common::ledger(store.pool()).await.is_empty());
    let first = seed_universe(&store, &state).await;
    let before = common::ledger(store.pool()).await;
    let mut envelope = common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1));
    envelope.observed_versions.insert(
        UbuId::new(ObjectType::Task),
        ubu_core::VersionRef::Version(7),
    );
    assert!(matches!(
        queries::persist_universe_state(store.pool(), &envelope, &state, AuthoritySource::System)
            .await
            .unwrap_err(),
        ubu_store::StoreError::PreconditionFailed { .. }
    ));
    assert_eq!(
        queries::get_current_state(store.pool(), state.id.as_str())
            .await
            .unwrap(),
        Some(first)
    );
    assert_eq!(common::ledger(store.pool()).await, before);
}

#[tokio::test]
async fn universe_ledger_failure_rolls_back_updated_object() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut state = populated_universe_state();
    let first = seed_universe(&store, &state).await;
    let before = common::ledger(store.pool()).await;
    common::fail_ledger_inserts(store.pool()).await;
    state.numeric_values.insert("energy".into(), 0.1);
    let envelope = common::envelope_for(state.id.as_str(), ubu_core::VersionRef::Version(1));
    let error =
        queries::persist_universe_state(store.pool(), &envelope, &state, AuthoritySource::System)
            .await
            .unwrap_err();
    assert!(error.to_string().contains("injected ledger failure"));
    assert_eq!(
        queries::get_current_state(store.pool(), state.id.as_str())
            .await
            .unwrap(),
        Some(first)
    );
    assert_eq!(common::ledger(store.pool()).await, before);
    assert_eq!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap(),
        None
    );
}
