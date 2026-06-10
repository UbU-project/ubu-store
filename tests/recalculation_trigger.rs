use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::log_record::NewLogRecord;
use ubu_store::{queries, recalculation, UbuStore};

#[tokio::test]
async fn queries_recalculation_triggers() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    queries::append_log_entry(
        store.pool(),
        NewLogRecord {
            id: UbuId::new(ObjectType::LogEntry).to_string(),
            event_type: "recalculation_requested".to_owned(),
            object_refs: json!([]),
            payload: json!({"reason": "test"}),
            provenance: json!({
                "createdAt": "2026-06-10T14:30:00Z",
                "authoritySource": "system"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("trigger log stored");

    let triggers = recalculation::recalculation_triggers(store.pool())
        .await
        .expect("triggers query");
    assert_eq!(triggers.len(), 1);
}
