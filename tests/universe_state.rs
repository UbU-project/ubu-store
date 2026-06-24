use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use ubu_core::core::{JsonScalar, UniverseState};
use ubu_core::id_registry::ObjectType;
use ubu_core::store::CandidateObject;
use ubu_core::{AuthoritySource, UbuId, UbuTimestamp};
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn admits_universe_state_and_round_trips_all_collections() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let payload = universe_state_payload(&state);

    let admitted = queries::admit_object(
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
async fn admits_universe_state_candidate_with_provenance_authority_source() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let state = populated_universe_state();
    let payload = universe_state_payload(&state);

    let admitted = queries::admit_candidate_object(
        store.pool(),
        CandidateObject {
            candidate_id: state.id.to_string(),
            object_type: ObjectType::UniverseState.as_str().to_owned(),
            payload: payload.clone(),
            submitted_at: UbuTimestamp::parse("2026-06-22T13:00:00Z").expect("valid submitted_at"),
            authority_source: AuthoritySource::User,
        },
        "default",
    )
    .await
    .expect("candidate admitted");

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

    let result = queries::admit_object(
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

    let result = queries::admit_object(
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

    queries::admit_object(
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

    let persisted =
        queries::persist_universe_state(store.pool(), &updated, AuthoritySource::System)
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

    let result =
        queries::persist_universe_state(store.pool(), &state, AuthoritySource::System).await;

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
