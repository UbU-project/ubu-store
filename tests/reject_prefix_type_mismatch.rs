use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn rejects_prefix_type_mismatch() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let task_id = UbuId::new(ObjectType::Task).to_string();
    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: task_id,
            object_type: "Objective".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({"title": "wrong type"}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}
