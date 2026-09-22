mod common;
use serde_json::{json, Value};
use ubu_core::{ObjectType, UbuError, UbuId, VersionRef};
use ubu_store::{models::object_record::NewObjectRecord, queries, StoreError, UbuStore};
fn record(kind: ObjectType, fields: Value) -> NewObjectRecord {
    let id = UbuId::new(kind).to_string();
    let mut payload = json!({"id":id,"status":"active","wrapper_metadata":{},"provenance":{"created_at":"2026-09-22T09:00:00Z","authority_source":"user"}});
    payload
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    NewObjectRecord {
        id,
        object_type: kind.as_str().into(),
        version: 1,
        status: "active".into(),
        compartment_label: "test".into(),
        payload,
        created_at: "2026-09-22T09:00:00Z".into(),
        updated_at: "2026-09-22T09:00:00Z".into(),
    }
}
fn routine() -> Value {
    json!({"mode":"evergreen","recurrence":{"timezone":"UTC","rule":{"kind":"daily"},"schedule_version":1},"routine_instance_template":{"title":"Synthetic","duration_estimate":{"type":"fixed","seconds":300},"nominal_start":"12:00:00","placement":"static","template_version":1}})
}
#[tokio::test]
async fn routine_admission_checks_each_cross_field_without_rejecting_wrapper_fields() {
    let store = UbuStore::in_memory().await.unwrap();
    common::admit_object(store.pool(), record(ObjectType::Objective, routine()))
        .await
        .unwrap();
    for (case, expected) in [
        (0, UbuError::RecurrenceRequiresEvergreen),
        (1, UbuError::RoutineTemplateRequiresRecurrence),
        (2, UbuError::RoutineObjectiveWithPriority),
        (3, UbuError::RoutineAfterSelfReference),
    ] {
        let mut r = record(ObjectType::Objective, routine());
        match case {
            0 => r.payload["mode"] = json!("one_time"),
            1 => {
                r.payload.as_object_mut().unwrap().remove("recurrence");
            }
            2 => r.payload["priority"] = json!(1),
            _ => {
                r.payload["routine_instance_template"]["after"] =
                    json!([{"objective_id":r.id,"offset_seconds":0}])
            }
        }
        assert!(
            matches!(common::admit_object(store.pool(),r).await,Err(StoreError::Core(error)) if error==expected)
        );
    }
}
#[tokio::test]
async fn occurrence_keys_are_unique_but_updates_and_absent_keys_are_allowed() {
    let store = UbuStore::in_memory().await.unwrap();
    let occurrence = json!({"routine_objective_id":UbuId::new(ObjectType::Objective),"local_date":"2026-09-22","key":"synthetic-key"});
    let r = record(ObjectType::Task, json!({"occurrence":occurrence}));
    let saved = common::admit_object(store.pool(), r.clone()).await.unwrap();
    let duplicate = record(ObjectType::Task, json!({"occurrence":occurrence}));
    assert!(
        matches!(common::admit_object(store.pool(),duplicate).await,Err(StoreError::DuplicateOccurrenceKey{key}) if key=="synthetic-key")
    );
    let envelope = common::envelope_for(&r.id, VersionRef::Version(saved.version as u64));
    assert_eq!(
        queries::admit_object(store.pool(), &envelope, r)
            .await
            .unwrap()
            .version,
        2
    );
    for _ in 0..2 {
        common::admit_object(store.pool(), record(ObjectType::Task, json!({})))
            .await
            .unwrap();
    }
    let mut r = record(ObjectType::Task, json!({"occurrence":occurrence}));
    r.payload["occurrence"]["key"] = json!("");
    assert!(matches!(
        common::admit_object(store.pool(), r).await,
        Err(StoreError::Core(UbuError::EmptyOccurrenceKey))
    ));
    // Bypass admission: the migration is an independent database backstop.
    let r = record(ObjectType::Task, json!({"occurrence":occurrence}));
    assert!(sqlx::query("INSERT INTO objects(id,object_type,version,status,compartment_label,payload_json,created_at,updated_at) VALUES (?, 'Task', 1, 'active', 'test', ?, ?, ?)")
        .bind(r.id).bind(r.payload.to_string()).bind(r.created_at).bind(r.updated_at).execute(store.pool()).await.is_err());
}
