use serde_json::Value;
use sqlx::SqlitePool;
use ubu_core::store::RecalculationTrigger;

use crate::errors::Result;
use crate::queries::query_recalculation_triggers;

pub async fn recalculation_triggers(pool: &SqlitePool) -> Result<Vec<RecalculationTrigger>> {
    let records = query_recalculation_triggers(pool).await?;
    records
        .into_iter()
        .map(|record| {
            let payload = serde_json::from_str::<Value>(&record.payload_json)?;
            validate_recalculation_trigger_payload(&payload)
        })
        .collect()
}

pub fn validate_recalculation_trigger_payload(value: &Value) -> Result<RecalculationTrigger> {
    Ok(serde_json::from_value(value.clone())?)
}
