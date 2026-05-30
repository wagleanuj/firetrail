//! Transport-agnostic error type for firetrail ops.
//!
//! `OpsError` is intentionally narrow: it has no HTTP status codes, no CLI
//! exit codes, and no user-facing formatting concerns. Adapters in ft-cli and
//! ft-ui translate it into their respective transports.

use ft_workspace::WorkspaceError;
use thiserror::Error;

/// Errors returned by every op in this crate.
#[derive(Debug, Error)]
pub enum OpsError {
    /// A referenced entity does not exist.
    #[error("{kind} not found: {id}")]
    NotFound {
        /// What kind of entity was looked up (e.g. `"ticket"`, `"memory"`).
        kind: String,
        /// The id used in the lookup.
        id: String,
    },

    /// The requested operation conflicts with existing state
    /// (e.g. duplicate, stale read, illegal transition).
    #[error("conflict: {reason}")]
    Conflict {
        /// Human-readable explanation of the conflict.
        reason: String,
    },

    /// The caller's identity is not permitted to perform this operation.
    #[error("permission denied: {reason}")]
    PermissionDenied {
        /// Reason the request was rejected.
        reason: String,
    },

    /// Input failed validation (shape, range, required field, etc.).
    #[error("validation failed on field `{field}`: {reason}")]
    Validation {
        /// Field name (dot-path for nested inputs).
        field: String,
        /// Why the field was invalid.
        reason: String,
    },

    /// An unexpected internal error — bugs, IO failures, corrupted state.
    /// Transport adapters generally map this to HTTP 500 / CLI exit 70.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl OpsError {
    /// Construct a [`OpsError::NotFound`] without ceremony.
    pub fn not_found(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self::NotFound {
            kind: kind.into(),
            id: id.into(),
        }
    }

    /// Construct a [`OpsError::Conflict`] without ceremony.
    pub fn conflict(reason: impl Into<String>) -> Self {
        Self::Conflict {
            reason: reason.into(),
        }
    }

    /// Construct a [`OpsError::PermissionDenied`] without ceremony.
    pub fn permission_denied(reason: impl Into<String>) -> Self {
        Self::PermissionDenied {
            reason: reason.into(),
        }
    }

    /// Construct a [`OpsError::Validation`] without ceremony.
    pub fn validation(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

/// Map a workspace-resolution error onto the transport-agnostic [`OpsError`].
///
/// The variant shapes line up one-to-one, so this is a lossless rename: a
/// missing root becomes [`OpsError::NotFound`], a broken invariant becomes
/// [`OpsError::Validation`], and an internal failure stays internal.
impl From<WorkspaceError> for OpsError {
    fn from(err: WorkspaceError) -> Self {
        match err {
            WorkspaceError::NotFound { entity, path } => OpsError::NotFound {
                kind: entity,
                id: path,
            },
            WorkspaceError::Validation { field, reason } => {
                OpsError::Validation { field, reason }
            }
            WorkspaceError::Internal(e) => OpsError::Internal(e),
        }
    }
}
