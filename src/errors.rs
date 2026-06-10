use thiserror::Error;

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Core(#[from] ubu_core::UbuError),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("unknown object type `{0}`")]
    UnknownObjectType(String),

    #[error("invalid JSON payload: {0}")]
    InvalidPayload(String),
}
