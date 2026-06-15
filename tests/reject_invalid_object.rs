use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn rejects_invalid_object_id() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: "bad_018f3c8e9b2a7c4d8f1e2a3b4c5d6e7f".to_owned(),
            object_type: "Task".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({"title": "bad"}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rejects_noncanonical_task_status() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    for status in ["canceled", "ready", "in_progress", "proposed", "blocked"] {
        let result = queries::admit_object(
            store.pool(),
            NewObjectRecord {
                id: UbuId::new(ObjectType::Task).to_string(),
                object_type: "Task".to_owned(),
                version: 1,
                status: status.to_owned(),
                compartment_label: "default".to_owned(),
                payload: json!({"title": "bad status"}),
                created_at: "2026-06-10T14:30:00Z".to_owned(),
                updated_at: "2026-06-10T14:30:00Z".to_owned(),
            },
        )
        .await;

        let err = result.expect_err("noncanonical status rejected");
        assert!(err.to_string().contains("UBU-D0227"));
    }
}

#[tokio::test]
async fn rejects_moot_task_without_payload_reason() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: UbuId::new(ObjectType::Task).to_string(),
            object_type: "Task".to_owned(),
            version: 1,
            status: "moot".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({"title": "missing moot reason"}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn admits_moot_task_with_payload_reason() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let task_id = UbuId::new(ObjectType::Task).to_string();
    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: task_id.clone(),
            object_type: "Task".to_owned(),
            version: 1,
            status: "moot".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": task_id,
                "title": "has moot reason",
                "status": "moot",
                "moot_reason_code": "duplicate",
                "provenance": {
                    "created_at": "2026-06-10T14:30:00Z",
                    "authority_source": "user"
                }
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_ok());
}
