use sqlx::SqlitePool;

use crate::db;
use crate::errors::Result;

#[derive(Clone)]
pub struct UbuStore {
    pool: SqlitePool,
}

impl UbuStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = db::init(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn in_memory() -> Result<Self> {
        Self::connect("sqlite::memory:").await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

pub async fn init_store(database_url: &str) -> Result<UbuStore> {
    UbuStore::connect(database_url).await
}
