//! Top-level error type for `ft-pr`.
//!
//! [`PrError`] surfaces conditions that *prevent* validation from running at
//! all (storage I/O blew up, refs do not resolve, regex compile failure).
//! Findings produced by individual rules are reported via
//! [`crate::PrReport`], not as `PrError`s.

use ft_git::GitError;
use ft_storage::StorageError;
use thiserror::Error;

/// Errors that abort validation before a report can be produced.
#[derive(Debug, Error)]
pub enum PrError {
    /// Git layer failed (ref resolution, diff walk, file read).
    #[error("git: {0}")]
    Git(#[from] GitError),

    /// Storage layer failed (read error not classified as missing-file).
    #[error("storage: {0}")]
    Storage(#[from] StorageError),

    /// JSON deserialization of a record at a ref failed.
    #[error("decode {path}: {source}")]
    Decode {
        /// Repo-relative path of the record file that failed to decode.
        path: String,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// One of the secret-scan patterns failed to compile.
    #[error("invalid secret pattern: {0}")]
    InvalidPattern(String),
}
