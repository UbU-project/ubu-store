mod common;

use serde_json::{json, Value};
use ubu_core::{ObjectType, UbuError, UbuId, VersionRef};
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, StoreError, UbuStore};

fn record(fields: Value) -> NewObjectRecord {
    let id = UbuId::new(ObjectType::Task).to_string();
    let mut payload = json!({"id": id, "title":"Mapping fields", "status":"active",
        "provenance":{"created_at":"2026-09-20T09:00:00Z","authority_source":"user"},
        "wrapper_metadata":{"retained":true}});
    payload
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    NewObjectRecord {
        id,
        object_type: "Task".into(),
        version: 1,
        status: "active".into(),
        compartment_label: "default".into(),
        payload,
        created_at: "2026-09-20T09:00:00Z".into(),
        updated_at: "2026-09-20T09:00:00Z".into(),
    }
}

async fn rejected(fields: Value) -> StoreError {
    let store = UbuStore::in_memory().await.unwrap();
    let record = record(fields);
    let id = record.id.clone();
    let error = common::admit_object(store.pool(), record)
        .await
        .unwrap_err();
    assert!(queries::get_current_state(store.pool(), &id)
        .await
        .unwrap()
        .is_none());
    assert!(common::ledger(store.pool()).await.is_empty());
    error
}

#[tokio::test]
async fn admits_static_noncapacity_and_categorized_tasks_with_wrapper_fields() {
    for fields in [
        json!({"static_window":{"start":"2026-09-21T10:00:00Z","end":"2026-09-21T11:00:00Z"},
            "duration_estimate":{"type":"fixed","seconds":1}}),
        json!({"occupies_capacity":false}),
        json!({"tags":["focus","notes"],"category_tag":"focus"}),
        json!({"static_window":{"start":"2026-09-21T10:00:00Z","end":"2026-09-21T11:00:00Z"},
            "occupies_capacity":false,"tags":["focus"],"category_tag":"focus"}),
        json!({}),
    ] {
        let store = UbuStore::in_memory().await.unwrap();
        let record = record(fields);
        let expected = record.payload.clone();
        let saved = common::admit_object(store.pool(), record).await.unwrap();
        let payload: Value = serde_json::from_str(&saved.payload_json).unwrap();
        assert_eq!(payload, expected);
        // The store deliberately accepts wrapper fields rather than deserializing
        // a whole Task. Remove that wrapper only to inspect the core defaults.
        let mut task_payload = payload;
        task_payload
            .as_object_mut()
            .unwrap()
            .remove("wrapper_metadata");
        let task: ubu_core::core::Task = serde_json::from_value(task_payload).unwrap();
        assert_eq!(
            task.occupies_capacity,
            expected
                .get("occupies_capacity")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        );
    }
}

#[tokio::test]
async fn rejects_non_increasing_static_windows_with_the_core_error() {
    for end in [
        "2026-09-21T09:59:59Z",
        "2026-09-21T10:00:00Z",
        "2026-09-21T06:00:00-04:00",
    ] {
        assert!(matches!(
            rejected(json!({"static_window":{"start":"2026-09-21T10:00:00Z","end":end}})).await,
            StoreError::Core(UbuError::InvalidTaskStaticWindow)
        ));
    }
}

#[tokio::test]
async fn rejects_missing_empty_or_case_mismatched_category_membership() {
    for fields in [
        json!({"category_tag":"focus"}),
        json!({"tags":[],"category_tag":"focus"}),
        json!({"tags":["focus"],"category_tag":"Focus"}),
        json!({"tags":["focus"],"category_tag":"missing"}),
        json!({"tags":[""],"category_tag":""}),
    ] {
        assert!(matches!(
            rejected(fields).await,
            StoreError::Core(UbuError::InvalidTaskCategoryTag)
        ));
    }
}

#[tokio::test]
async fn rejects_malformed_new_field_shapes_piecemeal() {
    for fields in [
        json!({"static_window":{"start":"2026-09-21T10:00:00Z"}}),
        json!({"static_window":{"start":"not-a-timestamp","end":"2026-09-21T11:00:00Z"}}),
        json!({"static_window":{"start":"2026-09-21T10:00:00Z","end":"2026-09-21T11:00:00Z","extra":0}}),
        json!({"occupies_capacity":"false"}),
        json!({"occupies_capacity":null}),
        json!({"category_tag":42}),
        json!({"category_tag":"focus","tags":[3]}),
    ] {
        assert!(matches!(rejected(fields).await, StoreError::Json(_)));
    }
}

#[tokio::test]
async fn invalid_category_update_preserves_existing_task_and_ledger() {
    let store = UbuStore::in_memory().await.unwrap();
    let mut record = record(json!({"tags":["focus"],"category_tag":"focus"}));
    let saved = common::admit_object(store.pool(), record.clone())
        .await
        .unwrap();
    let before = common::ledger(store.pool()).await;
    record.payload["tags"] = json!([]);
    let envelope = common::envelope_for(&record.id, VersionRef::Version(1));
    assert!(matches!(
        queries::admit_object(store.pool(), &envelope, record).await,
        Err(StoreError::Core(UbuError::InvalidTaskCategoryTag))
    ));
    assert_eq!(
        queries::get_current_state(store.pool(), &saved.id)
            .await
            .unwrap(),
        Some(saved)
    );
    assert_eq!(common::ledger(store.pool()).await, before);
}
