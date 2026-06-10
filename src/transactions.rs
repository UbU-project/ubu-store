use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::errors::Result;

pub async fn begin(pool: &SqlitePool) -> Result<Transaction<'_, Sqlite>> {
    Ok(pool.begin().await?)
}
