use sqlx::SqlitePool;

use crate::errors::Result;
use crate::models::log_record::LogRecord;
use crate::queries::query_recalculation_triggers;

pub async fn recalculation_triggers(pool: &SqlitePool) -> Result<Vec<LogRecord>> {
    query_recalculation_triggers(pool).await
}
