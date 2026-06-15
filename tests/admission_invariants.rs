use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::admission::object_type_from_str;
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{queries, UbuStore};

#[test]
fn accepts_new_core_object_type_names() {
    for (name, expected) in [
        ("Preference", ObjectType::Preference),
        ("Container", ObjectType::Container),
        ("UniverseState", ObjectType::UniverseState),
        ("Identity", ObjectType::Identity),
        ("Relationship", ObjectType::Relationship),
        ("ExternalEvent", ObjectType::ExternalEvent),
    ] {
        assert_eq!(object_type_from_str(name).expect("known type"), expected);
    }
}

#[test]
fn rejects_unknown_object_type_name() {
    assert!(object_type_from_str("Widget").is_err());
}

#[tokio::test]
async fn admits_preference_with_canonical_envelope() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let preference_id = UbuId::new(ObjectType::Preference).to_string();

    let admitted = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: preference_id.clone(),
            object_type: "Preference".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": preference_id,
                "name": "calendar_density",
                "value": "compact",
                "authority_source": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("preference admitted");

    assert_eq!(admitted.object_type, "Preference");
}

#[tokio::test]
async fn rejects_payload_without_canonical_id() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let preference_id = UbuId::new(ObjectType::Preference).to_string();

    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: preference_id,
            object_type: "Preference".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "name": "calendar_density",
                "value": "compact",
                "authority_source": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rejects_payload_id_that_does_not_match_record_id() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let record_id = UbuId::new(ObjectType::Preference).to_string();
    let payload_id = UbuId::new(ObjectType::Preference).to_string();

    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: record_id,
            object_type: "Preference".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": payload_id,
                "name": "calendar_density",
                "value": "compact",
                "authority_source": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rejects_non_positive_object_version() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let preference_id = UbuId::new(ObjectType::Preference).to_string();

    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: preference_id.clone(),
            object_type: "Preference".to_owned(),
            version: 0,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": preference_id,
                "name": "calendar_density",
                "value": "compact",
                "authority_source": "user"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rejects_payload_provenance_without_authority_source() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let task_id = UbuId::new(ObjectType::Task).to_string();

    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: task_id.clone(),
            object_type: "Task".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": task_id,
                "title": "missing authority",
                "status": "active",
                "provenance": {
                    "created_at": "2026-06-10T14:30:00Z"
                }
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rejects_preference_without_authority_source() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let preference_id = UbuId::new(ObjectType::Preference).to_string();

    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: preference_id.clone(),
            object_type: "Preference".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": preference_id,
                "name": "calendar_density",
                "value": "compact"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn admits_compartment_with_required_label_metadata() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let compartment_id = UbuId::new(ObjectType::Compartment).to_string();

    let admitted = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: compartment_id.clone(),
            object_type: "Compartment".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": compartment_id,
                "label": "default"
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("compartment admitted");

    assert_eq!(admitted.object_type, "Compartment");
}

#[tokio::test]
async fn rejects_compartment_without_required_label_metadata() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let compartment_id = UbuId::new(ObjectType::Compartment).to_string();

    let result = queries::admit_object(
        store.pool(),
        NewObjectRecord {
            id: compartment_id.clone(),
            object_type: "Compartment".to_owned(),
            version: 1,
            status: "active".to_owned(),
            compartment_label: "default".to_owned(),
            payload: json!({
                "id": compartment_id
            }),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
            updated_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await;

    assert!(result.is_err());
}
