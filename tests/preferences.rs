mod common;

use serde_json::{json, Value};
use ubu_store::{models::object_record::NewObjectRecord, queries, UbuStore};

fn pair(objectives: bool) -> Value {
    let kind = if objectives { "obj" } else { "task" };
    let mut payload = json!({
        "id":"pref_018f3c8e9b2a7c4d8f1e2a3b4c5d6e7f",
        "order":"a_preferred_to_b", "acquired_method":"user_defined",
        "acquired_date":"2026-09-21T12:00:00Z", "enabled":true,
        "provenance":{"created_at":"2026-09-21T12:00:00Z", "authority_source":"user"}
    });
    let names = if objectives {
        ["objective_a", "objective_b"]
    } else {
        ["task_a", "task_b"]
    };
    payload[names[0]] = json!(format!("{kind}_018f3c8e9b2a7c4d8f1e2a3b4c5d6e7f"));
    payload[names[1]] = json!(format!("{kind}_018f3c8e9b2a7c4d8f1e2a3b4c5d6e80"));
    payload
}

async fn admit(payload: Value) -> Result<(), ubu_store::StoreError> {
    let store = UbuStore::in_memory().await.unwrap();
    let id = payload["id"].as_str().unwrap().to_owned();
    let record = NewObjectRecord {
        id: id.clone(),
        object_type: "Preference".into(),
        version: 1,
        status: "active".into(),
        compartment_label: "default".into(),
        payload,
        created_at: "2026-09-21T12:00:00Z".into(),
        updated_at: "2026-09-21T12:00:00Z".into(),
    };
    match common::admit_object(store.pool(), record).await {
        Ok(_) => Ok(()),
        Err(error) => {
            assert!(queries::get_current_state(store.pool(), &id)
                .await
                .unwrap()
                .is_none());
            assert!(common::ledger(store.pool()).await.is_empty());
            Err(error)
        }
    }
}

#[tokio::test]
async fn admits_task_and_objective_pairs_without_legacy_authority_fields() {
    admit(pair(false)).await.unwrap();
    admit(pair(true)).await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_preferences_without_admitting_or_logging_them() {
    let mut mixed = pair(false);
    mixed["objective_a"] = pair(true)["objective_a"].clone();
    let mut same = pair(false);
    same["task_b"] = same["task_a"].clone();
    let mut no_provenance = pair(false);
    no_provenance.as_object_mut().unwrap().remove("provenance");
    let mut extra = pair(false);
    extra["legacy_wrapper"] = json!(true);
    for invalid in [mixed, same, no_provenance, extra] {
        assert!(admit(invalid).await.is_err());
    }
}
