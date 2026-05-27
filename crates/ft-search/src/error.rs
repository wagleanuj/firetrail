//! Error type for `ft-search`.

/// Errors that can arise from search operations.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// Underlying `SQLite` operation failed.
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    /// I/O failure opening the index database file.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Caller supplied an embedding whose length does not match
    /// [`crate::EMBEDDING_DIM`].
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// Expected dimension (`EMBEDDING_DIM`).
        expected: usize,
        /// Dimension actually supplied.
        actual: usize,
    },
    /// Caller asked for vector search but the `sqlite-vec` extension is not
    /// loaded in this build.
    #[error("vector search unavailable: sqlite-vec feature is disabled")]
    VectorUnavailable,
    /// Logical inconsistency detected (malformed row, bad enum string, etc.).
    #[error("integrity check failed: {0}")]
    Integrity(String),
}
