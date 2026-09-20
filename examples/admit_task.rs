use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::{
    AuthoritySource, CausalityIssuer, DeviceId, EnvelopeRequest, LocalIssuer, UbuId, UbuTimestamp,
    VersionRef,
};
use ubu_store::models::object_record::NewObjectRecord;
use ubu_store::{init_store, queries};

#[tokio::main]
async fn main() -> ubu_store::Result<()> {
    let store = init_store("sqlite::memory:").await?;
    let now = "2026-06-10T14:30:00Z".to_owned();
    let task = NewObjectRecord {
        id: UbuId::new(ObjectType::Task).to_string(),
        object_type: "Task".to_owned(),
        version: 1,
        status: "active".to_owned(),
        compartment_label: "default".to_owned(),
        payload: json!({"title": "Example task"}),
        created_at: now.clone(),
        updated_at: now,
    };
    // Restore this identifier from registration material in a real installation.
    let issuer = LocalIssuer::new(DeviceId::parse("example-registered-device")?);
    let envelope = issuer.issue(EnvelopeRequest {
        observed_versions: [(UbuId::parse(&task.id)?, VersionRef::Absent)].into(),
        actor_identity_id: UbuId::new(ObjectType::Identity),
        authority_source: AuthoritySource::User,
        effective_time: UbuTimestamp::parse(&task.updated_at)?,
        observed_policy_versions: None,
        execution_context: None,
    })?;
    queries::admit_object(store.pool(), &envelope, task).await?;
    Ok(())
}
