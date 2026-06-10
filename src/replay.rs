use sqlx::SqlitePool;

use crate::errors::Result;
use crate::models::log_record::LogRecord;
use crate::queries::get_object_history;

pub async fn replay_state(pool: &SqlitePool, object_id: &str) -> Result<Vec<LogRecord>> {
    get_object_history(pool, object_id).await
}
