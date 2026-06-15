use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn admits_valid_task_object() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let now = "2026-06-10T14:30:00Z".to_owned();
    let id = UbuId::new(ObjectType::Task).to_string();
    let admitted = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: id.clone(),
            object_type: "Task".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": id,
                "title": "ship scaffold",
                "status": "active",
                "provenance": {
                    "created_at": "2026-06-10T14:30:00Z",
                    "authority_source": "user"
                }
            }),
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .expect("object admitted");

    assert_eq!(admitted.id, id);
    assert_eq!(admitted.object_type, "Task");
}
