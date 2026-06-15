use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::store::TriggerType;
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
            payload: json!({
                "triggered_at": "2026-06-10T14:30:00Z",
                "trigger_type": "task_completed",
                "note": "test"
            }),
            provenance: json!({
                "created_at": "2026-06-10T14:30:00Z",
                "authority_source": "system"
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
    assert_eq!(triggers[0].trigger_type, TriggerType::TaskCompleted);
}

#[tokio::test]
async fn rejects_recalculation_trigger_with_free_form_reason() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let result = queries::append_log_entry(
        store.pool(),
        NewLogRecord {
            id: UbuId::new(ObjectType::LogEntry).to_string(),
            event_type: "recalculation_requested".to_owned(),
            object_refs: json!([]),
            payload: json!({
                "triggered_at": "2026-06-10T14:30:00Z",
                "trigger_type": "task_completed",
                "reason": "free form is no longer accepted"
            }),
            provenance: json!({
                "created_at": "2026-06-10T14:30:00Z",
                "authority_source": "system"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}
