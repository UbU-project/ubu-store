use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::log_record::NewLogRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn appends_log_entry() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let log_id = UbuId::new(ObjectType::LogEntry).to_string();
    let object_id = UbuId::new(ObjectType::Task).to_string();
    let log = queries::append_log_entry(
        store.pool(),
        NewLogRecord {
            id: log_id.clone(),
            event_type: "object_admitted".to_owned(),
            object_refs: json!([object_id]),
            payload: json!({"ok": true}),
            provenance: json!({
                "createdAt": "2026-06-10T14:30:00Z",
                "authoritySource": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("log appended");

    assert_eq!(log.id, log_id);
}
