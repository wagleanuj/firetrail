//! Error types for `ft-index`.

use ft_storage::StorageError;

/// Errors that can arise from index operations.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// `SQLite` operation failed.
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    /// Schema migration failed (forward-incompatible or scripted failure).
    #[error("schema migration failed: {0}")]
    Migration(String),
    /// Underlying storage failure.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    /// I/O failure (creating the index directory, etc.).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Logical inconsistency detected in the index or its inputs.
    #[error("integrity check failed: {0}")]
    Integrity(String),
    /// JSON parse failure when decoding stored row data.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
