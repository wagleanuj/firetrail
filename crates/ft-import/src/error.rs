//! Error variants for the import surface.

use std::path::PathBuf;

use ft_core::CoreError;
use ft_history::HistoryError;
use ft_storage::StorageError;

/// Errors returned by `ft-import` public functions.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Markdown body was empty or contained no usable content.
    #[error("empty input: {0}")]
    Empty(String),

    /// Parser could not locate a required structural element (e.g. an H1
    /// title on a runbook).
    #[error("parse: {0}")]
    Parse(String),

    /// Wrapped `ft-core` failure (typically hash / schema).
    #[error("core: {0}")]
    Core(#[from] CoreError),

    /// Wrapped `ft-storage` failure during import or promotion.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),

    /// Wrapped `ft-history` failure while maintaining a record's history
    /// chain (e.g. appending the promotion audit entry).
    #[error("history: {0}")]
    History(#[from] HistoryError),

    /// I/O while walking an import directory or reading a file.
    #[error("io reading {path}: {source}")]
    Io {
        /// Offending path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// External adapter (Jira / Confluence) failure.
    #[error("adapter: {0}")]
    Adapter(String),

    /// Caller asked to promote a record that is not currently quarantined.
    #[error("not quarantined: {0}")]
    NotQuarantined(String),
}
