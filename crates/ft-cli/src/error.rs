//! CLI error types and their mapping to documented exit codes.
//!
//! The CLI surfaces a small, stable set of error kinds; every subcommand
//! handler maps its domain errors into one of the variants below. The exit
//! codes match the table in `docs/components/ft-cli.md`.

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

/// Stable symbolic kind for an error. Matches `error.kind` in JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Bad user input (exit 1).
    UserError,
    /// Record / workspace / branch not found (exit 2).
    NotFound,
    /// Conflict: stale data, concurrent claim, hash mismatch (exit 3).
    Conflict,
    /// `.firetrail/` is missing or unreadable (exit 4).
    NotInitialized,
    /// Internal bug or unexpected I/O failure (exit 5).
    Internal,
}

impl ErrorKind {
    /// Numeric exit code that matches this kind.
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorKind::UserError => 1,
            ErrorKind::NotFound => 2,
            ErrorKind::Conflict => 3,
            ErrorKind::NotInitialized => 4,
            ErrorKind::Internal => 5,
        }
    }
}

/// All recoverable errors surfaced by the CLI.
#[derive(Debug, Error)]
pub enum CliError {
    /// The user supplied invalid arguments or the command refused for a
    /// validation reason (e.g. AC incomplete).
    #[error("{message}")]
    UserError {
        /// Which command produced the error (for output framing).
        command: String,
        /// Human-readable message.
        message: String,
        /// Optional machine-readable details for JSON output.
        details: serde_json::Value,
    },

    /// The workspace has not been initialised yet.
    #[error("workspace not initialised at {path}: run `firetrail init`")]
    NotInitialized {
        /// Command name for output framing.
        command: String,
        /// Path that was checked.
        path: PathBuf,
    },

    /// A target record / file was not found.
    #[error("not found: {what}")]
    NotFound {
        /// Command name for output framing.
        command: String,
        /// What was being looked up.
        what: String,
    },

    /// Stale data or a concurrent modification was detected.
    #[error("conflict: {message}")]
    Conflict {
        /// Command name for output framing.
        command: String,
        /// Human-readable message.
        message: String,
    },

    /// An unexpected internal failure. Treated as exit 5; prints a traceback
    /// when `--verbose` is set.
    #[error("internal error: {message}")]
    Internal {
        /// Command name for output framing.
        command: String,
        /// Human-readable message.
        message: String,
    },
}

impl CliError {
    /// The kind of this error.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            CliError::UserError { .. } => ErrorKind::UserError,
            CliError::NotInitialized { .. } => ErrorKind::NotInitialized,
            CliError::NotFound { .. } => ErrorKind::NotFound,
            CliError::Conflict { .. } => ErrorKind::Conflict,
            CliError::Internal { .. } => ErrorKind::Internal,
        }
    }

    /// Numeric exit code.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        self.kind().exit_code()
    }

    /// Command name for output framing.
    #[must_use]
    pub fn command(&self) -> &str {
        match self {
            CliError::UserError { command, .. }
            | CliError::NotInitialized { command, .. }
            | CliError::NotFound { command, .. }
            | CliError::Conflict { command, .. }
            | CliError::Internal { command, .. } => command,
        }
    }

    /// Optional machine-readable details for JSON output.
    #[must_use]
    pub fn details(&self) -> serde_json::Value {
        match self {
            CliError::UserError { details, .. } => details.clone(),
            CliError::NotInitialized { path, .. } => {
                serde_json::json!({ "path": path.display().to_string() })
            }
            CliError::NotFound { what, .. } => serde_json::json!({ "what": what }),
            CliError::Conflict { .. } | CliError::Internal { .. } => serde_json::Value::Null,
        }
    }

    /// Construct an internal error from any `Display` source.
    pub fn internal(command: impl Into<String>, message: impl std::fmt::Display) -> Self {
        CliError::Internal {
            command: command.into(),
            message: message.to_string(),
        }
    }

    /// Construct a user error.
    pub fn user(command: impl Into<String>, message: impl Into<String>) -> Self {
        CliError::UserError {
            command: command.into(),
            message: message.into(),
            details: serde_json::Value::Null,
        }
    }
}

/// Adapt a workspace-resolution failure into a CLI error.
///
/// All variants are framed under the `"workspace"` command. In practice only
/// the `?`-propagated `runtime_dir`/`daemon_socket_path` paths reach here, and
/// they produce [`WorkspaceError::Internal`] — preserving the previous
/// `CliError::Internal { command: "workspace", .. }` behaviour and exit code 5.
/// Discovery helpers (`locate` / `require_initialised`) still build their own
/// command-specific `CliError`s in `crate::workspace`, so their messages are
/// unchanged.
impl From<ft_workspace::WorkspaceError> for CliError {
    fn from(err: ft_workspace::WorkspaceError) -> Self {
        use ft_workspace::WorkspaceError as WE;
        match err {
            WE::NotFound { entity, path } => CliError::NotFound {
                command: "workspace".to_string(),
                what: format!("{entity}: {path}"),
            },
            WE::Validation { reason, .. } => CliError::UserError {
                command: "workspace".to_string(),
                message: reason,
                details: serde_json::Value::Null,
            },
            WE::Internal(e) => CliError::internal("workspace", e),
        }
    }
}
