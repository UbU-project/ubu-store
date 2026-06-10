use serde_json::Value;
use ubu_core::id_registry::ObjectType;
use ubu_core::{Provenance, UbuId, UbuTimestamp};

use crate::compartment_gate::validate_compartment_label;
use crate::errors::{Result, StoreError};
use crate::models::object_record::NewObjectRecord;
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
    serde_json::to_string(&record.payload)?;
    Ok(())
}

pub fn validate_provenance_json(value: &Value) -> Result<Provenance> {
    validate_provenance_value(value)
}
