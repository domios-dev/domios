use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemStoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
}

pub type MemStoreResult<T> = Result<T, MemStoreError>;
