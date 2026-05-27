//! Local declaration of the `Storage` contract that the index consumes.
//!
//! The real implementation lives in `ft-storage`. To keep `ft-index` buildable
//! independently of the storage crate's M1 schedule, the trait is mirrored
//! here. The shape is intentionally minimal — only what the index needs.
//! When `ft-storage` exports a canonical trait, these types become a
//! re-export and the trait alias bridges existing impls.

use std::path::{Path, PathBuf};

use ft_core::{Record, RecordId};

/// Errors returned by a [`Storage`] backend.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Underlying I/O failure.
    #[error("io: {0}")]
    Io(String),
    /// JSON parse / shape failure.
    #[error("parse: {0}")]
    Parse(String),
    /// Requested record not found.
    #[error("not found: {0}")]
    NotFound(String),
    /// Catch-all for backend-specific failures.
    #[error("other: {0}")]
    Other(String),
}

/// Filter applied when iterating storage.
///
/// At M1 the only filter is "include closed/archived records or not".
/// Future milestones add scope, kind, and last-updated-since filters.
#[derive(Debug, Default, Clone)]
pub struct StorageFilter {
    /// Include records whose status is `Closed`, `Deferred`, or `Archived`.
    pub include_closed: bool,
}

/// Read-side view of the JSON-in-Git record store.
///
/// `ft-index` uses this trait to populate and refresh the `SQLite` index.
/// Implementations are provided by `ft-storage` (the embedded mode is the
/// only one shipping at M1).
pub trait Storage: Send + Sync {
    /// Iterate every record matching `filter`.
    ///
    /// The iterator yields `(record, on-disk path)` pairs. The path is used
    /// by the index to record `file_path` / `file_mtime` for incremental
    /// refresh.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered while reading the store.
    #[allow(clippy::type_complexity, clippy::iter_not_returning_iterator)]
    fn iter(
        &self,
        filter: StorageFilter,
    ) -> Result<Box<dyn Iterator<Item = Result<(Record, PathBuf), StorageError>> + '_>, StorageError>;

    /// Read a single record by id.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if the id does not exist, or the
    /// underlying read/parse error otherwise.
    fn read(&self, id: &RecordId) -> Result<(Record, PathBuf), StorageError>;

    /// Read a single record by its on-disk path.
    ///
    /// Used by incremental refresh: the post-checkout hook hands us a set of
    /// changed paths.
    ///
    /// # Errors
    ///
    /// Returns the underlying read/parse error.
    fn read_path(&self, path: &Path) -> Result<Record, StorageError>;
}
