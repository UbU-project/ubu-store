use serde_json::json;
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
