use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn rejects_invalid_compartment_label() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: UbuId::new(ObjectType::Task).to_string(),
            object_type: "Task".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "!bad".to_owned(),
            payload: json!({"title": "bad compartment"}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}
