use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::log_record::NewLogRecord;
use ubu_store::{queries, replay, UbuStore};

#[tokio::test]
async fn replays_object_history_from_logs() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let object_id = UbuId::new(ObjectType::Task).to_string();
    queries::append_log_entry(
        store.pool(),
        NewLogRecord {
            id: UbuId::new(ObjectType::LogEntry).to_string(),
            event_type: "object_admitted".to_owned(),
            object_refs: json!([object_id.clone()]),
            payload: json!({"version": 1}),
            provenance: json!({
                "created_at": "2026-06-10T14:30:00Z",
                "authority_source": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("log appended");

    let history = replay::replay_state(store.pool(), &object_id)
        .await
        .expect("history reads");
    assert_eq!(history.len(), 1);
}
