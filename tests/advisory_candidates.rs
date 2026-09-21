mod common;

use serde_json::{json, Value};
use sqlx::SqlitePool;
use ubu_core::{
    AdvisoryCandidate, AdvisoryCandidateId, CandidateLifecycleState as State, MutationEnvelope,
    ObjectType, ResurfaceTrigger as Trigger, RetentionPolicy, SuppressionDecision, UbuError, UbuId,
    UbuTimestamp, VersionRef,
};
use ubu_store::api::admission::{
    admit_advisory_candidate, reject_advisory_candidate, store_advisory_candidate,
    transition_advisory_candidate, RejectionInput,
};
use ubu_store::api::review::{
    find_suppression_record, get_advisory_candidate, list_candidate_decision_events, review_queue,
    CandidateDecisionEvent, CandidateRecord,
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
        payload: json!({"id": id, "title": "Proposed focus task", "status": "active",
            "provenance": {"created_at": "2026-09-19T09:00:00Z", "authority_source": "user"}}),
        created_at: "2026-09-19T09:00:00Z".into(),
        updated_at: "2026-09-19T09:00:00Z".into(),
    }
}

async fn setup() -> (UbuStore, AdvisoryCandidate, NewObjectRecord) {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task();
    let candidate = common::candidate_for(&record);
    store_advisory_candidate(store.pool(), &common::append_envelope(), candidate.clone())
        .await
        .unwrap();
    (store, candidate, record)
}

fn rejection_input() -> RejectionInput {
    RejectionInput {
        rejection_reason_or_user_correction: "Do not infer this tag.".into(),
        retention_policy: RetentionPolicy::PurgePayload,
        evidence_hashes_or_source_fingerprints: vec!["source-fingerprint:task:v1".into()],
        suppression_key: None,
    }
}

async fn snapshot(pool: &SqlitePool) -> Value {
    let candidates: Vec<CandidateRecord> =
        sqlx::query_as("SELECT * FROM advisory_candidates ORDER BY advisory_candidate_id")
            .fetch_all(pool)
            .await
            .unwrap();
    let events: Vec<CandidateDecisionEvent> =
        sqlx::query_as("SELECT * FROM candidate_decision_events ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap();
    let suppressions: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT suppression_key, advisory_candidate_id, payload_json, decided_at FROM suppression_records ORDER BY suppression_key")
        .fetch_all(pool).await.unwrap();
    let objects: Vec<ObjectRecord> = sqlx::query_as("SELECT * FROM objects ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap();
    json!({"candidates": candidates, "events": events, "suppressions": suppressions,
        "objects": objects, "ledger": common::ledger(pool).await, "canonical_counts": canonical_counts(pool).await})
}

async fn canonical_counts(pool: &SqlitePool) -> Vec<i64> {
    let mut counts = Vec::new();
    for table in [
        "objects",
        "logs",
        "external_references",
        "plans",
        "calendars",
        "snapshots",
        "projection_previews",
        "projection_results",
        "worker_submissions",
    ] {
        counts.push(
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(pool)
                .await
                .unwrap(),
        );
    }
    counts
}

async fn row(pool: &SqlitePool, id: &AdvisoryCandidateId) -> CandidateRecord {
    get_advisory_candidate(pool, id).await.unwrap().unwrap()
}

#[tokio::test]
async fn fresh_migration_has_all_three_tables_and_indexes() {
    let store = UbuStore::in_memory().await.unwrap();
    for (name, kind) in [
        ("advisory_candidates", "table"),
        ("candidate_decision_events", "table"),
        ("suppression_records", "table"),
        ("idx_advisory_candidates_queue", "index"),
        ("idx_advisory_candidates_suppression", "index"),
        ("idx_candidate_decision_events_candidate", "index"),
    ] {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = ? AND type = ?")
                .bind(name)
                .bind(kind)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(count, 1, "{name}");
    }
    assert_eq!(review_queue(store.pool()).await.unwrap(), vec![]);
    assert!(
        get_advisory_candidate(store.pool(), &AdvisoryCandidateId::generate())
            .await
            .unwrap()
            .is_none()
    );
    assert!(find_suppression_record(store.pool(), "missing")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn proposed_candidate_never_enters_admitted_reads_or_canonical_tables() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    // These assertions would have failed before P1B-5: the legacy candidate
    // writer put proposed content into objects as active canonical state.
    assert!(queries::get_current_state(store.pool(), id.as_str())
        .await
        .unwrap()
        .is_none());
    assert!(queries::get_current_state(store.pool(), &object.id)
        .await
        .unwrap()
        .is_none());
    assert!(queries::query_active_tasks(store.pool())
        .await
        .unwrap()
        .is_empty());
    assert!(queries::query_external_references(store.pool(), None)
        .await
        .unwrap()
        .is_empty());
    assert!(queries::get_object_history(store.pool(), &object.id)
        .await
        .unwrap()
        .is_empty());
    assert!(queries::query_recalculation_triggers(store.pool())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(canonical_counts(store.pool()).await, vec![0; 9]);
    assert_eq!(row(store.pool(), id).await.candidate().unwrap(), candidate);
    assert!(list_candidate_decision_events(store.pool(), id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn lifecycle_deferral_resurfacing_then_admission_records_each_decision() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let defer = common::append_envelope();
    let deferred =
        transition_advisory_candidate(store.pool(), &defer, id, 1, State::Deferred, None)
            .await
            .unwrap();
    assert_eq!(
        (deferred.lifecycle_state.as_str(), deferred.version),
        ("deferred", 2)
    );
    let resurface = common::append_envelope();
    let resurfaced = transition_advisory_candidate(
        store.pool(),
        &resurface,
        id,
        2,
        State::Resurfaced,
        Some(Trigger::MateriallyNewEvidence),
    )
    .await
    .unwrap();
    assert_eq!(
        (resurfaced.lifecycle_state.as_str(), resurfaced.version),
        ("resurfaced", 3)
    );
    let admission = common::envelope_for(&object.id, VersionRef::Absent);
    let (admitted_candidate, admitted_object) =
        admit_advisory_candidate(store.pool(), &admission, id, 3, object)
            .await
            .unwrap();
    assert_eq!(
        (
            admitted_candidate.lifecycle_state.as_str(),
            admitted_candidate.version
        ),
        ("admitted", 4)
    );
    let events = list_candidate_decision_events(store.pool(), id)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
    for (index, (from, to, env)) in [
        ("proposed", "deferred", &defer),
        ("deferred", "resurfaced", &resurface),
        ("resurfaced", "admitted", &admission),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(events[index].from_state, from);
        assert_eq!(events[index].to_state, to);
        assert_eq!(events[index].observed_candidate_version, (index + 1) as i64);
        assert_eq!(
            serde_json::from_str::<MutationEnvelope>(&events[index].envelope_json).unwrap(),
            *env
        );
    }
    assert_eq!(
        events[1].resurface_trigger.as_deref(),
        Some("materially_new_evidence")
    );
    let links = resurfaced.candidate().unwrap().links;
    assert_eq!(
        links.prior_deferral_ref.as_deref(),
        Some(events[0].id.as_str())
    );
    assert_eq!(
        links.resurfacing_ref.as_deref(),
        Some(events[1].id.as_str())
    );
    assert_eq!(
        events[2].resulting_object_id.as_deref(),
        Some(admitted_object.id.as_str())
    );
    assert!(review_queue(store.pool()).await.unwrap().is_empty());
}

#[tokio::test]
async fn deferred_cannot_be_admitted_directly_and_archived_cannot_transition() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        id,
        1,
        State::Deferred,
        None,
    )
    .await
    .unwrap();
    let before = snapshot(store.pool()).await;
    let error = transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        id,
        2,
        State::Admitted,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        StoreError::Core(UbuError::InvalidCandidateTransition {
            current: State::Deferred,
            next: State::Admitted
        })
    ));
    assert!(admit_advisory_candidate(
        store.pool(),
        &common::envelope_for(&object.id, VersionRef::Absent),
        id,
        2,
        object
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
    transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        id,
        2,
        State::Archived,
        None,
    )
    .await
    .unwrap();
    let before = snapshot(store.pool()).await;
    for next in [
        State::Proposed,
        State::Deferred,
        State::Resurfaced,
        State::Admitted,
        State::Rejected,
        State::Superseded,
        State::Archived,
    ] {
        let trigger = (next == State::Resurfaced).then_some(Trigger::UserRequest);
        assert!(matches!(
            transition_advisory_candidate(
                store.pool(),
                &common::append_envelope(),
                id,
                3,
                next,
                trigger
            )
            .await,
            Err(StoreError::Core(UbuError::InvalidCandidateTransition {
                current: State::Archived,
                ..
            }))
        ));
        assert_eq!(snapshot(store.pool()).await, before);
    }
}

#[tokio::test]
async fn stale_observations_and_invalid_triggers_leave_all_storage_unchanged() {
    let (store, candidate, _) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let before = snapshot(store.pool()).await;
    assert!(matches!(
        transition_advisory_candidate(
            store.pool(),
            &common::append_envelope(),
            id,
            0,
            State::Deferred,
            None
        )
        .await,
        Err(StoreError::PreconditionFailed { .. })
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    assert!(transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        id,
        1,
        State::Deferred,
        Some(Trigger::UserRequest)
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
    transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        id,
        1,
        State::Deferred,
        None,
    )
    .await
    .unwrap();
    let before = snapshot(store.pool()).await;
    assert!(transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        id,
        2,
        State::Resurfaced,
        None
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn rejection_records_suppression_and_decision_without_canonical_state() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let envelope = common::append_envelope();
    let input = rejection_input();
    let mut expected_candidate = candidate.clone();
    expected_candidate.lifecycle_state = State::Rejected;
    let suppression = expected_candidate
        .suppression_record(SuppressionDecision {
            deciding_actor_identity_id: envelope.actor_identity_id.clone(),
            authority_source: envelope.authority_source,
            decided_at: envelope.effective_time,
            rejection_reason_or_user_correction: input.rejection_reason_or_user_correction.clone(),
            retention_policy: input.retention_policy,
            evidence_hashes_or_source_fingerprints: input
                .evidence_hashes_or_source_fingerprints
                .clone(),
        })
        .unwrap();
    let rejected = reject_advisory_candidate(store.pool(), &envelope, id, 1, input.clone())
        .await
        .unwrap();
    assert_eq!(
        (rejected.lifecycle_state.as_str(), rejected.version),
        ("rejected", 2)
    );
    assert_eq!(
        find_suppression_record(store.pool(), &suppression.suppression_key)
            .await
            .unwrap(),
        Some(suppression.clone())
    );
    assert!(queries::get_current_state(store.pool(), &object.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(canonical_counts(store.pool()).await, vec![0; 9]);
    let events = list_candidate_decision_events(store.pool(), id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].from_state.as_str(), events[0].to_state.as_str()),
        ("proposed", "rejected")
    );
    assert_eq!(events[0].resulting_object_id, None);
    assert_eq!(
        rejected.candidate().unwrap().links.rejection_ref.as_deref(),
        Some(events[0].id.as_str())
    );
    let before = snapshot(store.pool()).await;
    assert_eq!(
        reject_advisory_candidate(store.pool(), &envelope, id, 1, input)
            .await
            .unwrap(),
        rejected
    );
    assert_eq!(snapshot(store.pool()).await, before);
}

async fn reject_without_key(mut candidate: AdvisoryCandidate) -> String {
    let store = UbuStore::in_memory().await.unwrap();
    candidate.suppression_key = None;
    store_advisory_candidate(store.pool(), &common::append_envelope(), candidate.clone())
        .await
        .unwrap();
    let envelope = common::append_envelope();
    let input = rejection_input();
    let rejected = reject_advisory_candidate(
        store.pool(),
        &envelope,
        &candidate.advisory_candidate_id,
        1,
        input.clone(),
    )
    .await
    .unwrap();
    let key = rejected.suppression_key.clone().unwrap();
    let expected_key = String::from_utf8(ubu_core::canonical_payload_bytes(&json!({
        "candidate_kind": candidate.candidate_kind,
        "normalized_proposal": candidate.normalized_proposal,
        "target_refs": candidate.target_refs,
    })))
    .unwrap();
    assert_eq!(key, expected_key);
    let expected = rejected
        .candidate()
        .unwrap()
        .suppression_record(SuppressionDecision {
            deciding_actor_identity_id: envelope.actor_identity_id.clone(),
            authority_source: envelope.authority_source,
            decided_at: envelope.effective_time,
            rejection_reason_or_user_correction: input.rejection_reason_or_user_correction.clone(),
            retention_policy: input.retention_policy,
            evidence_hashes_or_source_fingerprints: input
                .evidence_hashes_or_source_fingerprints
                .clone(),
        })
        .unwrap();
    assert_eq!(
        find_suppression_record(store.pool(), &key).await.unwrap(),
        Some(expected)
    );
    assert_eq!(canonical_counts(store.pool()).await, vec![0; 9]);
    let before = snapshot(store.pool()).await;
    assert_eq!(
        reject_advisory_candidate(
            store.pool(),
            &envelope,
            &candidate.advisory_candidate_id,
            1,
            input,
        )
        .await
        .unwrap(),
        rejected
    );
    assert_eq!(snapshot(store.pool()).await, before);
    key
}

#[tokio::test]
async fn proposed_candidates_derive_deterministic_target_scoped_keys() {
    let object = task();
    let first = common::candidate_for(&object);
    let mut second = common::candidate_for(&object);
    // Object member order is not part of proposal identity.
    second.normalized_proposal =
        serde_json::from_str(r#"{"tag":"focus","operation":"add_tag"}"#).unwrap();
    let key = reject_without_key(first).await;
    assert_eq!(key, reject_without_key(second).await);
    let different_target = common::candidate_for(&task());
    assert_ne!(key, reject_without_key(different_target).await);
}

#[tokio::test]
async fn rejection_key_precedence_keeps_existing_or_accepts_supplied() {
    for existing in [None, Some("existing-key".to_owned())] {
        let store = UbuStore::in_memory().await.unwrap();
        let mut candidate = common::candidate_for(&task());
        candidate.suppression_key = existing.clone();
        store_advisory_candidate(store.pool(), &common::append_envelope(), candidate.clone())
            .await
            .unwrap();
        let mut input = rejection_input();
        input.suppression_key = Some(existing.clone().unwrap_or_else(|| "caller-key".into()));
        let rejected = reject_advisory_candidate(
            store.pool(),
            &common::append_envelope(),
            &candidate.advisory_candidate_id,
            1,
            input.clone(),
        )
        .await
        .unwrap();
        assert_eq!(rejected.suppression_key, input.suppression_key);
    }
}

#[tokio::test]
async fn admission_is_atomic_links_the_object_and_replays_without_writes() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let envelope = common::envelope_for(&object.id, VersionRef::Absent);
    let result = admit_advisory_candidate(store.pool(), &envelope, id, 1, object.clone())
        .await
        .unwrap();
    assert_eq!(
        (result.0.lifecycle_state.as_str(), result.0.version),
        ("admitted", 2)
    );
    assert_eq!(result.1.version, 1);
    assert_eq!(
        queries::get_current_state(store.pool(), &object.id)
            .await
            .unwrap(),
        Some(result.1.clone())
    );
    let events = list_candidate_decision_events(store.pool(), id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].resulting_object_id.as_deref(),
        Some(object.id.as_str())
    );
    let ledger = queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ledger.result_object_id, object.id);
    assert_eq!(ledger.result_version, 1);
    assert_eq!(
        ledger.canonical_payload.as_bytes(),
        ubu_core::canonical_payload_bytes(&object.payload)
    );
    assert_eq!(
        serde_json::from_str::<MutationEnvelope>(&ledger.envelope_json).unwrap(),
        envelope
    );
    let before = snapshot(store.pool()).await;
    assert_eq!(
        admit_advisory_candidate(store.pool(), &envelope, id, 1, object.clone())
            .await
            .unwrap(),
        result
    );
    assert_eq!(snapshot(store.pool()).await, before);
    let mut changed = object;
    changed.payload["title"] = json!("different");
    assert!(matches!(
        admit_advisory_candidate(store.pool(), &envelope, id, 1, changed).await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn canonical_precondition_failure_leaves_proposed_candidate_and_no_event_or_envelope() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let before = snapshot(store.pool()).await;
    let envelope = common::envelope_for(&object.id, VersionRef::Version(1));
    assert!(matches!(
        admit_advisory_candidate(store.pool(), &envelope, id, 1, object.clone()).await,
        Err(StoreError::PreconditionFailed { .. })
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    assert_eq!(row(store.pool(), id).await.lifecycle_state, "proposed");
    assert!(queries::get_current_state(store.pool(), &object.id)
        .await
        .unwrap()
        .is_none());
    assert!(list_candidate_decision_events(store.pool(), id)
        .await
        .unwrap()
        .is_empty());
    assert!(
        queries::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn storing_replays_one_row_and_changed_candidate_payload_conflicts() {
    let store = UbuStore::in_memory().await.unwrap();
    let candidate = common::candidate_for(&task());
    let envelope = common::append_envelope();
    let first = store_advisory_candidate(store.pool(), &envelope, candidate.clone())
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    assert_eq!(
        store_advisory_candidate(store.pool(), &envelope, candidate.clone())
            .await
            .unwrap(),
        first
    );
    assert_eq!(snapshot(store.pool()).await, before);
    assert_eq!(review_queue(store.pool()).await.unwrap().len(), 1);
    let mut changed = candidate;
    changed.normalized_proposal["tag"] = json!("different");
    assert!(matches!(
        store_advisory_candidate(store.pool(), &envelope, changed).await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn queue_excludes_inactive_states_and_orders_by_order_then_creation_time() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut expected = Vec::new();
    for (state, order, time) in [
        (State::Proposed, Some(3), "2026-09-19T08:00:00Z"),
        (State::Resurfaced, Some(1), "2026-09-19T10:00:00Z"),
        (State::Proposed, Some(1), "2026-09-19T09:00:00Z"),
        (State::Proposed, None, "2026-09-19T11:00:00Z"),
        (State::Deferred, Some(0), "2026-09-19T09:00:00Z"),
        (State::Rejected, Some(0), "2026-09-19T09:00:00Z"),
        (State::Superseded, Some(0), "2026-09-19T09:00:00Z"),
        (State::Archived, Some(0), "2026-09-19T09:00:00Z"),
        (State::Admitted, Some(0), "2026-09-19T09:00:00Z"),
    ] {
        let object = task();
        let mut candidate = common::candidate_for(&object);
        candidate.review_order = order;
        candidate.proposed_at = UbuTimestamp::parse(time).unwrap();
        let id = &candidate.advisory_candidate_id;
        store_advisory_candidate(store.pool(), &common::append_envelope(), candidate.clone())
            .await
            .unwrap();
        match state {
            State::Proposed => {}
            State::Resurfaced => {
                transition_advisory_candidate(
                    store.pool(),
                    &common::append_envelope(),
                    id,
                    1,
                    State::Deferred,
                    None,
                )
                .await
                .unwrap();
                transition_advisory_candidate(
                    store.pool(),
                    &common::append_envelope(),
                    id,
                    2,
                    State::Resurfaced,
                    Some(Trigger::UserRequest),
                )
                .await
                .unwrap();
            }
            State::Rejected => {
                let env = common::append_envelope();
                reject_advisory_candidate(store.pool(), &env, id, 1, rejection_input())
                    .await
                    .unwrap();
            }
            State::Admitted => {
                admit_advisory_candidate(
                    store.pool(),
                    &common::envelope_for(&object.id, VersionRef::Absent),
                    id,
                    1,
                    object,
                )
                .await
                .unwrap();
            }
            _ => {
                transition_advisory_candidate(
                    store.pool(),
                    &common::append_envelope(),
                    id,
                    1,
                    state,
                    None,
                )
                .await
                .unwrap();
            }
        }
        if state.is_active_queue() {
            expected.push((order, time, id.as_str().to_owned()));
        }
    }
    expected.sort();
    assert_eq!(
        review_queue(store.pool())
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.advisory_candidate_id)
            .collect::<Vec<_>>(),
        expected
            .into_iter()
            .map(|(_, _, id)| id)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn initial_state_version_and_candidate_validation_are_enforced() {
    let store = UbuStore::in_memory().await.unwrap();
    let before = snapshot(store.pool()).await;
    for state in [
        State::Deferred,
        State::Resurfaced,
        State::Admitted,
        State::Rejected,
        State::Superseded,
        State::Archived,
    ] {
        let mut candidate = common::candidate_for(&task());
        candidate.lifecycle_state = state;
        if state == State::Resurfaced {
            candidate.links.prior_deferral_ref = Some("earlier".into());
            candidate.links.resurface_trigger = Some(Trigger::UserRequest);
        }
        assert!(matches!(
            store_advisory_candidate(store.pool(), &common::append_envelope(), candidate).await,
            Err(StoreError::InvalidInitialCandidateState { .. })
        ));
        assert_eq!(snapshot(store.pool()).await, before);
    }
    for version in [0, u64::MAX] {
        let mut candidate = common::candidate_for(&task());
        candidate.version = version;
        assert!(
            store_advisory_candidate(store.pool(), &common::append_envelope(), candidate)
                .await
                .is_err()
        );
        assert_eq!(snapshot(store.pool()).await, before);
    }
    let mut candidate = common::candidate_for(&task());
    candidate.confidence = Some(f64::NAN);
    assert!(
        store_advisory_candidate(store.pool(), &common::append_envelope(), candidate)
            .await
            .is_err()
    );
    assert_eq!(snapshot(store.pool()).await, before);
    let mut candidate = common::candidate_for(&task());
    candidate.version = 7;
    assert_eq!(
        store_advisory_candidate(store.pool(), &common::append_envelope(), candidate)
            .await
            .unwrap()
            .version,
        7
    );
}

#[tokio::test]
async fn generic_transition_cannot_bypass_admission_or_suppression() {
    let (store, candidate, _) = setup().await;
    let before = snapshot(store.pool()).await;
    for next in [State::Admitted, State::Rejected] {
        assert!(matches!(
            transition_advisory_candidate(
                store.pool(),
                &common::append_envelope(),
                &candidate.advisory_candidate_id,
                1,
                next,
                None
            )
            .await,
            Err(StoreError::DedicatedCandidateWriterRequired { .. })
        ));
        assert_eq!(snapshot(store.pool()).await, before);
    }
}

#[tokio::test]
async fn candidate_decisions_enforce_canonical_read_preconditions_and_replay_first() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let before = snapshot(store.pool()).await;
    let bad = common::envelope_for(&object.id, VersionRef::Version(1));
    assert!(matches!(
        transition_advisory_candidate(store.pool(), &bad, id, 1, State::Deferred, None).await,
        Err(StoreError::PreconditionFailed { .. })
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    let mut env = common::envelope_for(&object.id, VersionRef::Absent);
    let result = transition_advisory_candidate(store.pool(), &env, id, 1, State::Deferred, None)
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    env.observed_versions
        .insert(UbuId::parse(&object.id).unwrap(), VersionRef::Version(99));
    assert_eq!(
        transition_advisory_candidate(store.pool(), &env, id, 1, State::Deferred, None)
            .await
            .unwrap(),
        result
    );
    assert_eq!(snapshot(store.pool()).await, before);
    assert!(matches!(
        transition_advisory_candidate(store.pool(), &env, id, 1, State::Archived, None).await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn candidate_and_canonical_keys_are_device_global_and_cannot_fake_admission() {
    let (store, candidate, object) = setup().await;
    let id = &candidate.advisory_candidate_id;
    let env = common::envelope_for(&object.id, VersionRef::Absent);
    queries::admit_object(store.pool(), &env, object.clone())
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    assert!(matches!(
        admit_advisory_candidate(store.pool(), &env, id, 1, object.clone()).await,
        Err(StoreError::ReplayTargetMismatch { .. })
    ));
    assert!(matches!(
        transition_advisory_candidate(store.pool(), &env, id, 1, State::Deferred, None).await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    let env = common::append_envelope();
    transition_advisory_candidate(store.pool(), &env, id, 1, State::Deferred, None)
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    assert!(matches!(
        queries::admit_object(store.pool(), &env, object).await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn admission_replay_must_match_the_candidate_and_observed_version() {
    let (store, candidate, object) = setup().await;
    let env = common::envelope_for(&object.id, VersionRef::Absent);
    admit_advisory_candidate(
        store.pool(),
        &env,
        &candidate.advisory_candidate_id,
        1,
        object.clone(),
    )
    .await
    .unwrap();
    let other = common::candidate_for(&object);
    store_advisory_candidate(store.pool(), &common::append_envelope(), other.clone())
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    for (id, version) in [
        (&candidate.advisory_candidate_id, 2),
        (&other.advisory_candidate_id, 1),
    ] {
        assert!(matches!(
            admit_advisory_candidate(store.pool(), &env, id, version, object.clone()).await,
            Err(StoreError::ReplayTargetMismatch { .. })
        ));
        assert_eq!(snapshot(store.pool()).await, before);
    }
}

#[tokio::test]
async fn decision_insert_failure_rolls_back_canonical_object_and_ledger() {
    let (store, candidate, object) = setup().await;
    sqlx::query("CREATE TRIGGER reject_decision BEFORE INSERT ON candidate_decision_events BEGIN SELECT RAISE(ABORT, 'injected decision failure'); END")
        .execute(store.pool()).await.unwrap();
    let before = snapshot(store.pool()).await;
    let env = common::envelope_for(&object.id, VersionRef::Absent);
    assert!(admit_advisory_candidate(
        store.pool(),
        &env,
        &candidate.advisory_candidate_id,
        1,
        object
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
    let env = common::append_envelope();
    assert!(reject_advisory_candidate(
        store.pool(),
        &env,
        &candidate.advisory_candidate_id,
        1,
        rejection_input()
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn ledger_failure_rolls_back_all_four_candidate_writers() {
    let (store, candidate, object) = setup().await;
    common::fail_ledger_inserts(store.pool()).await;
    let before = snapshot(store.pool()).await;
    assert!(store_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        common::candidate_for(&task())
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
    assert!(transition_advisory_candidate(
        store.pool(),
        &common::append_envelope(),
        &candidate.advisory_candidate_id,
        1,
        State::Deferred,
        None
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
    let env = common::append_envelope();
    assert!(reject_advisory_candidate(
        store.pool(),
        &env,
        &candidate.advisory_candidate_id,
        1,
        rejection_input()
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
    assert!(admit_advisory_candidate(
        store.pool(),
        &common::envelope_for(&object.id, VersionRef::Absent),
        &candidate.advisory_candidate_id,
        1,
        object
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn conflicting_key_duplicate_key_and_changed_replay_cannot_overwrite_corrections() {
    let (store, candidate, _) = setup().await;
    let env = common::append_envelope();
    let original = rejection_input();
    let before = snapshot(store.pool()).await;
    let mut invalid = original.clone();
    invalid.suppression_key = Some("unrelated".into());
    assert!(matches!(
        reject_advisory_candidate(
            store.pool(),
            &env,
            &candidate.advisory_candidate_id,
            1,
            invalid
        )
        .await,
        Err(StoreError::SuppressionKeyConflict)
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    reject_advisory_candidate(
        store.pool(),
        &env,
        &candidate.advisory_candidate_id,
        1,
        original.clone(),
    )
    .await
    .unwrap();
    let before = snapshot(store.pool()).await;
    let mut changed = original;
    changed.rejection_reason_or_user_correction = "different".into();
    assert!(matches!(
        reject_advisory_candidate(
            store.pool(),
            &env,
            &candidate.advisory_candidate_id,
            1,
            changed
        )
        .await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    let mut other = common::candidate_for(&task());
    other.suppression_key = candidate.suppression_key;
    store_advisory_candidate(store.pool(), &common::append_envelope(), other.clone())
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    let env = common::append_envelope();
    assert!(reject_advisory_candidate(
        store.pool(),
        &env,
        &other.advisory_candidate_id,
        1,
        rejection_input()
    )
    .await
    .is_err());
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn admission_can_update_existing_object_and_candidate_version_overflow_rolls_it_back() {
    let (store, candidate, mut object) = setup().await;
    common::admit_object(store.pool(), object.clone())
        .await
        .unwrap();
    object.payload["title"] = json!("Accepted change");
    let env = common::envelope_for(&object.id, VersionRef::Version(1));
    let (_, updated) = admit_advisory_candidate(
        store.pool(),
        &env,
        &candidate.advisory_candidate_id,
        1,
        object.clone(),
    )
    .await
    .unwrap();
    assert_eq!(updated.version, 2);
    assert_eq!(
        serde_json::from_str::<Value>(&updated.payload_json).unwrap()["title"],
        "Accepted change"
    );
    let mut exhausted = common::candidate_for(&object);
    exhausted.version = i64::MAX as u64;
    store_advisory_candidate(store.pool(), &common::append_envelope(), exhausted.clone())
        .await
        .unwrap();
    let before = snapshot(store.pool()).await;
    let env = common::envelope_for(&object.id, VersionRef::Version(2));
    assert!(matches!(
        admit_advisory_candidate(
            store.pool(),
            &env,
            &exhausted.advisory_candidate_id,
            exhausted.version,
            object
        )
        .await,
        Err(StoreError::ObjectVersionExhausted { .. })
    ));
    assert_eq!(snapshot(store.pool()).await, before);
}

#[tokio::test]
async fn canonical_ledger_view_cannot_expose_candidate_request_payloads() {
    let store = UbuStore::in_memory().await.unwrap();
    let object = task();
    let candidate = common::candidate_for(&object);
    let envelope = common::append_envelope();
    store_advisory_candidate(store.pool(), &envelope, candidate.clone())
        .await
        .unwrap();
    assert!(
        ubu_store::api::query::get_recorded_mutation(store.pool(), &envelope.mutation_key())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(common::ledger(store.pool()).await.len(), 1);
    let before = snapshot(store.pool()).await;
    // The private lookup still observes this key across both table categories.
    assert!(matches!(
        queries::admit_object(store.pool(), &envelope, object).await,
        Err(StoreError::Core(UbuError::IdempotencyKeyConflict { .. }))
    ));
    assert_eq!(snapshot(store.pool()).await, before);
    let replay = store_advisory_candidate(store.pool(), &envelope, candidate)
        .await
        .unwrap();
    assert_eq!(replay.lifecycle_state, "proposed");
    assert_eq!(snapshot(store.pool()).await, before);
}
