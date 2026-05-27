//! Error variants returned by the storage layer.

use std::path::PathBuf;

use ft_core::{CoreError, RecordId};
use ft_git::GitError;

/// Errors returned by [`crate::Storage`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Record id was not present on disk (or at the requested ref).
    #[error("record not found: {0}")]
    NotFound(RecordId),

    /// A record file existed but failed schema validation, parse, or other
    /// structural checks.
    #[error("invalid record on disk at {path}: {reason}")]
    Invalid {
        /// The path the bad data was read from.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },

    /// The `state_hash` field embedded in a record file does not match the
    /// hash recomputed from the file's body.
    #[error("hash mismatch for {id}: file says {file_hash}, recompute says {recomputed}")]
    HashMismatch {
        /// Id of the offending record.
        id: RecordId,
        /// The hash recorded on disk.
        file_hash: String,
        /// The hash recomputed by [`ft_core::state_hash`].
        recomputed: String,
    },

    /// Workspace `.firetrail/records/` directory is missing and the caller
    /// used `open` rather than `init`.
    #[error("workspace not initialized: {0}")]
    NotInitialized(PathBuf),

    /// Underlying I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// `ft-git` reported an error.
    #[error("git: {0}")]
    Git(#[from] GitError),

    /// `ft-core` reported an error (typically schema or hash failure).
    #[error("core: {0}")]
    Core(#[from] CoreError),

    /// JSON serialization or parsing error.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}
