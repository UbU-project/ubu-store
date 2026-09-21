mod common;

use serde_json::{json, Value};
use ubu_core::id_registry::ObjectType;
use ubu_core::{CorrelationGroupViolation, DurationEstimateViolation, UbuError, UbuId};
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, StoreError, UbuStore};

fn task_record(payload_fields: Value) -> NewObjectRecord {
    let id = UbuId::new(ObjectType::Task).to_string();
    let mut payload = json!({
        "id": id,
        "title": "Estimate task duration",
        "status": "active",
        "provenance": {
            "created_at": "2026-06-10T14:30:00Z",
            "authority_source": "user"
        }
    });
    payload
        .as_object_mut()
        .expect("task payload is an object")
        .extend(
            payload_fields
                .as_object()
                .expect("payload fields are an object")
                .clone(),
        );

    NewObjectRecord {
        id,
        object_type: "Task".to_owned(),
        version: 1,
        status: "active".to_owned(),
        compartment_label: "default".to_owned(),
        payload,
        created_at: "2026-06-10T14:30:00Z".to_owned(),
        updated_at: "2026-06-10T14:30:00Z".to_owned(),
    }
}

async fn admit_and_query(record: NewObjectRecord) -> Value {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let id = record.id.clone();
    let expected = record.payload.clone();

    common::admit_object(store.pool(), record)
        .await
        .expect("task is admitted");
    let queried = queries::get_current_state(store.pool(), &id)
        .await
        .expect("query succeeds")
        .expect("task is stored");
    let payload: Value = serde_json::from_str(&queried.payload_json).expect("payload is JSON");
    assert_eq!(payload, expected);
    payload
}

#[tokio::test]
async fn admits_and_round_trips_fixed_duration_estimate() {
    let fields = json!({
        "duration_estimate": {
            "type": "fixed",
            "seconds": 900
        }
    });

    let payload = admit_and_query(task_record(fields.clone())).await;
    assert_eq!(payload["duration_estimate"], fields["duration_estimate"]);
}

#[tokio::test]
async fn admits_and_round_trips_shifted_duration_and_correlation_groups() {
    let fields = json!({
        "duration_estimate": {
            "type": "shifted_lognormal_p95",
            "min_seconds": 300,
            "mode_seconds": 900,
            "p95_seconds": 3600
        },
        "correlation_groups": [
            { "group": "shared-test-environment", "strength": 0.75 },
            { "group": "release-readiness", "strength": 1 }
        ]
    });

    let payload = admit_and_query(task_record(fields.clone())).await;
    assert_eq!(payload["duration_estimate"], fields["duration_estimate"]);
    assert_eq!(payload["correlation_groups"], fields["correlation_groups"]);
}

#[tokio::test]
async fn admits_and_round_trips_task_without_estimate_fields() {
    let payload = admit_and_query(task_record(json!({}))).await;
    assert!(payload.get("duration_estimate").is_none());
    assert!(payload.get("correlation_groups").is_none());
}

#[tokio::test]
async fn rejects_invalid_three_point_duration_ordering() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let record = task_record(json!({
        "duration_estimate": {
            "type": "shifted_lognormal_p95",
            "min_seconds": 900,
            "mode_seconds": 300,
            "p95_seconds": 3600
        }
    }));

    let error = common::admit_object(store.pool(), record)
        .await
        .expect_err("invalid ordering is rejected");
    assert!(error.to_string().contains("min_seconds < mode_seconds"));
    assert!(matches!(
        error,
        StoreError::Core(UbuError::InvalidTaskDurationEstimate {
            violation: DurationEstimateViolation::NotStrictlyIncreasing,
        })
    ));
}

async fn rejected(fields: Value) -> StoreError {
    let store = UbuStore::in_memory().await.unwrap();
    let record = task_record(fields);
    let id = record.id.clone();
    let error = common::admit_object(store.pool(), record)
        .await
        .unwrap_err();
    assert!(queries::get_current_state(store.pool(), &id)
        .await
        .unwrap()
        .is_none());
    error
}

#[tokio::test]
async fn rejects_zero_seconds_with_core_violation() {
    let error = rejected(json!({"duration_estimate": {"type": "fixed", "seconds": 0}})).await;
    assert!(error
        .to_string()
        .contains("seconds must be greater than zero"));
    assert!(matches!(
        error,
        StoreError::Core(UbuError::InvalidTaskDurationEstimate {
            violation: DurationEstimateViolation::ZeroSeconds,
        })
    ));
}

#[tokio::test]
async fn rejects_each_non_increasing_duration_with_core_violation() {
    for (min, mode, p95) in [(10, 10, 20), (11, 10, 20), (0, 10, 10), (0, 11, 10)] {
        let error = rejected(json!({"duration_estimate": {
            "type": "shifted_lognormal_p95", "min_seconds": min,
            "mode_seconds": mode, "p95_seconds": p95,
        }}))
        .await;
        assert!(matches!(
            error,
            StoreError::Core(UbuError::InvalidTaskDurationEstimate {
                violation: DurationEstimateViolation::NotStrictlyIncreasing,
            })
        ));
    }
}

#[tokio::test]
async fn rejects_unknown_fields_through_core_types_serde_contract() {
    // Unknown fields remain serde errors, distinct from semantic core violations.
    for fields in [
        json!({"duration_estimate": {"type": "fixed", "seconds": 1, "unexpected": true}}),
        json!({"duration_estimate": {"type": "shifted_lognormal_p95", "min_seconds": 0,
            "mode_seconds": 1, "p95_seconds": 2, "unexpected": true}}),
        json!({"correlation_groups": [{"group": "g", "strength": 0.5, "unexpected": true}]}),
    ] {
        match rejected(fields).await {
            StoreError::Json(error) => {
                assert!(error.to_string().contains("unknown field `unexpected`"))
            }
            error => panic!("expected serde unknown-field error, got {error:?}"),
        }
    }
}

#[tokio::test]
async fn rejects_correlation_strength_and_duplicate_names_with_core_violations() {
    for strength in [-0.1, 1.1] {
        let error =
            rejected(json!({"correlation_groups": [{"group": "g", "strength": strength}]})).await;
        assert!(error
            .to_string()
            .contains("strength must be between zero and one"));
        assert!(matches!(
            error,
            StoreError::Core(UbuError::InvalidTaskCorrelationGroup {
                violation: CorrelationGroupViolation::StrengthOutOfRange,
            })
        ));
    }
    let error = rejected(json!({"correlation_groups": [
        {"group": "g", "strength": 0.0}, {"group": "g", "strength": 1.0},
    ]}))
    .await;
    assert!(error.to_string().contains("names must be unique"));
    assert!(matches!(
        error,
        StoreError::Core(UbuError::InvalidTaskCorrelationGroup {
            violation: CorrelationGroupViolation::DuplicateName,
        })
    ));
}

#[tokio::test]
async fn accepts_duration_and_correlation_boundary_values() {
    for fields in [
        json!({"duration_estimate": {"type": "fixed", "seconds": 1}}),
        json!({"duration_estimate": {"type": "fixed", "seconds": u64::MAX}}),
        json!({"duration_estimate": {"type": "shifted_lognormal_p95", "min_seconds": 0,
            "mode_seconds": 1, "p95_seconds": 2}}),
        json!({"correlation_groups": []}),
        json!({"correlation_groups": [{"group": "", "strength": 0.0}, {"group": "g", "strength": 1.0}]}),
    ] {
        admit_and_query(task_record(fields)).await;
    }
}
