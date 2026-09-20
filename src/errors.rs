use thiserror::Error;

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("precondition failed for `{object_id}`: expected {expected}, actual {actual}")]
    PreconditionFailed {
        object_id: String,
        expected: String,
        actual: String,
    },

    #[error("missing target precondition for `{object_id}`")]
    MissingTargetPrecondition { object_id: String },

    #[error("object `{object_id}` cannot advance beyond SQLite version {version}")]
    ObjectVersionExhausted { object_id: String, version: u64 },

    #[error("recorded mutation target `{object_id}` has no current state")]
    RecordedMutationObjectMissing { object_id: String },

    #[error("recorded mutation target `{object_id}` is not a result in `{expected_table}`")]
    ReplayTargetMismatch {
        object_id: String,
        expected_table: &'static str,
    },

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
