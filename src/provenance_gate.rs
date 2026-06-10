use serde_json::Value;
use ubu_core::Provenance;

use crate::errors::Result;

pub fn validate_provenance_value(value: &Value) -> Result<Provenance> {
    Ok(serde_json::from_value(value.clone())?)
}
