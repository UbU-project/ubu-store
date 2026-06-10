use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::calendar_record::NewCalendarRecord;
use ubu_store::models::plan_record::NewPlanRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn stores_plan_and_calendar() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let plan_id = UbuId::new(ObjectType::Plan).to_string();
    queries::store_plan(
        store.pool(),
        NewPlanRecord {
            id: plan_id.clone(),
            request_id: "request-1".to_owned(),
            status: "draft".to_owned(),
            payload: json!({"steps": []}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("plan stored");

    let calendar = queries::store_calendar(
        store.pool(),
        NewCalendarRecord {
            id: UbuId::new(ObjectType::Calendar).to_string(),
            plan_id,
            window_start: "2026-06-10T15:00:00Z".to_owned(),
            window_end: "2026-06-10T16:00:00Z".to_owned(),
            payload: json!({"items": []}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("calendar stored");

    assert_eq!(calendar.window_start, "2026-06-10T15:00:00Z");
}
