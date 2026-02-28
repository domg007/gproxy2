#[derive(Debug, thiserror::Error)]
pub enum AdminApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("storage query failed: {0}")]
    Storage(String),
    #[error("storage write queue failed: {0}")]
    Queue(String),
}

impl From<gproxy_storage::StorageWriteQueueError> for AdminApiError {
    fn from(value: gproxy_storage::StorageWriteQueueError) -> Self {
        Self::Queue(value.to_string())
    }
}

impl From<sea_orm::DbErr> for AdminApiError {
    fn from(value: sea_orm::DbErr) -> Self {
        Self::Storage(value.to_string())
    }
}
