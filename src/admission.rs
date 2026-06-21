use std::collections::HashSet;

use serde_json::{Map, Value};
use ubu_core::core::TaskStatus;
use ubu_core::id_registry::ObjectType;
use ubu_core::{AuthoritySource, Provenance, UbuId, UbuTimestamp};

use crate::compartment_gate::validate_compartment_label;
use crate::errors::{Result, StoreError};
use crate::models::object_record::NewObjectRecord;
use crate::models::task_record::{TaskCorrelationGroup, TaskDurationEstimate};
use crate::provenance_gate::validate_provenance_value;

pub fn object_type_from_str(value: &str) -> Result<ObjectType> {
    match value {
        "Task" => Ok(ObjectType::Task),
        "Objective" => Ok(ObjectType::Objective),
        "Plan" => Ok(ObjectType::Plan),
        "LogEntry" => Ok(ObjectType::LogEntry),
        "ExternalReference" => Ok(ObjectType::ExternalReference),
        "Compartment" => Ok(ObjectType::Compartment),
        "Snapshot" => Ok(ObjectType::Snapshot),
        "AutomationWorker" => Ok(ObjectType::AutomationWorker),
        "ProjectionPreview" => Ok(ObjectType::ProjectionPreview),
        "Calendar" => Ok(ObjectType::Calendar),
        "Preference" => Ok(ObjectType::Preference),
        "Container" => Ok(ObjectType::Container),
        "UniverseState" => Ok(ObjectType::UniverseState),
        "Identity" => Ok(ObjectType::Identity),
        "Relationship" => Ok(ObjectType::Relationship),
        "ExternalEvent" => Ok(ObjectType::ExternalEvent),
        other => Err(StoreError::UnknownObjectType(other.to_owned())),
    }
}

pub fn validate_object_id_for_type(id: &str, object_type: &str) -> Result<UbuId> {
    let parsed_id = UbuId::parse(id.to_owned())?;
    let expected = object_type_from_str(object_type)?;
    parsed_id.require_object_type(expected)?;
    Ok(parsed_id)
}

pub fn validate_object_record(record: &NewObjectRecord) -> Result<()> {
    validate_object_id_for_type(&record.id, &record.object_type)?;
    validate_compartment_label(&record.compartment_label)?;
    UbuTimestamp::parse(&record.created_at)?;
    UbuTimestamp::parse(&record.updated_at)?;
    validate_object_version(record)?;
    validate_task_lifecycle_status(record)?;
    validate_canonical_object_payload(record)?;
    serde_json::to_string(&record.payload)?;
    Ok(())
}

pub fn validate_provenance_json(value: &Value) -> Result<Provenance> {
    validate_provenance_value(value)
}

fn validate_task_lifecycle_status(record: &NewObjectRecord) -> Result<()> {
    if record.object_type != ObjectType::Task.as_str() {
        return Ok(());
    }

    if matches!(
        record.status.as_str(),
        "canceled" | "ready" | "in_progress" | "proposed" | "blocked"
    ) {
        return Err(invalid_payload(format!(
            "UBU-D0227: `{}` is a derived or noncanonical Task status and must not be persisted",
            record.status
        )));
    }

    let status: TaskStatus = serde_json::from_value(Value::String(record.status.clone()))?;
    let has_moot_reason_code = record.payload.get("moot_reason_code").is_some();

    match (status, has_moot_reason_code) {
        (TaskStatus::Moot, false) => Err(invalid_payload(
            "UBU-D0227: task status `moot` requires moot_reason_code",
        )),
        (TaskStatus::Moot, true) => Ok(()),
        (_, true) => Err(invalid_payload(format!(
            "UBU-D0227: task status `{}` forbids moot_reason_code",
            record.status
        ))),
        (_, false) => Ok(()),
    }
}

fn validate_object_version(record: &NewObjectRecord) -> Result<()> {
    if record.version < 1 {
        return Err(invalid_payload(
            "object_version must be present as a positive store version",
        ));
    }
    Ok(())
}

fn validate_canonical_object_payload(record: &NewObjectRecord) -> Result<()> {
    let payload = record
        .payload
        .as_object()
        .ok_or_else(|| invalid_payload("canonical object payload must be a JSON object"))?;

    let payload_id = payload
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_payload("canonical object payload must include id"))?;
    validate_object_id_for_type(payload_id, &record.object_type)?;
    if payload_id != record.id {
        return Err(invalid_payload(
            "canonical object payload id must match admitted object id",
        ));
    }

    let object_type = object_type_from_str(&record.object_type)?;
    validate_payload_lifecycle_status(record, payload)?;
    validate_task_estimate_fields(object_type, payload)?;
    validate_payload_provenance(object_type, payload)?;
    validate_payload_authority_source(object_type, payload)?;
    validate_payload_compartment_metadata(object_type, payload)?;

    Ok(())
}

fn validate_task_estimate_fields(
    object_type: ObjectType,
    payload: &Map<String, Value>,
) -> Result<()> {
    if object_type != ObjectType::Task {
        return Ok(());
    }

    if let Some(value) = payload.get("duration_estimate") {
        let estimate: TaskDurationEstimate = serde_json::from_value(value.clone())?;
        estimate.validate()?;
    }

    if let Some(value) = payload.get("correlation_groups") {
        let groups: Vec<TaskCorrelationGroup> = serde_json::from_value(value.clone())?;
        let mut names = HashSet::with_capacity(groups.len());
        for group in groups {
            if !(0.0..=1.0).contains(&group.strength) {
                return Err(invalid_payload(
                    "core/task schema: correlation group strength must be between zero and one",
                ));
            }
            if !names.insert(group.group) {
                return Err(invalid_payload(
                    "core/task schema: correlation group names must be unique",
                ));
            }
        }
    }

    Ok(())
}

fn validate_payload_lifecycle_status(
    record: &NewObjectRecord,
    payload: &serde_json::Map<String, Value>,
) -> Result<()> {
    if record.object_type != ObjectType::Task.as_str() {
        return Ok(());
    }

    let payload_status = payload
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_payload("Task payload must include canonical status"))?;
    if payload_status != record.status {
        return Err(invalid_payload(
            "Task payload status must match admitted object status",
        ));
    }
    Ok(())
}

fn validate_payload_provenance(
    object_type: ObjectType,
    payload: &serde_json::Map<String, Value>,
) -> Result<()> {
    let provenance = payload.get("provenance");

    if matches!(
        object_type,
        ObjectType::Task | ObjectType::Objective | ObjectType::ExternalReference
    ) && provenance.is_none()
    {
        return Err(invalid_payload(
            "canonical object payload must include provenance",
        ));
    }

    if let Some(provenance) = provenance {
        validate_provenance_json(provenance)?;
    }

    Ok(())
}

fn validate_payload_authority_source(
    object_type: ObjectType,
    payload: &serde_json::Map<String, Value>,
) -> Result<()> {
    if !matches!(object_type, ObjectType::LogEntry | ObjectType::Preference) {
        return Ok(());
    }

    let authority_source = payload
        .get("authority_source")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_payload("canonical object payload must include authority_source"))?;
    serde_json::from_value::<AuthoritySource>(Value::String(authority_source.to_owned()))?;
    Ok(())
}

fn validate_payload_compartment_metadata(
    object_type: ObjectType,
    payload: &serde_json::Map<String, Value>,
) -> Result<()> {
    if object_type != ObjectType::Compartment {
        return Ok(());
    }

    let label = payload
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_payload("Compartment payload must include label"))?;
    validate_compartment_label(label)?;
    Ok(())
}

fn invalid_payload(message: impl Into<String>) -> StoreError {
    StoreError::InvalidPayload(message.into())
}
