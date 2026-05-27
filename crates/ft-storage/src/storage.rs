//! The canonical [`Storage`] trait.
//!
//! Downstream crates (`ft-index`, `ft-history`, `ft-pr`, `ft-cli`, ...)
//! depend on this trait rather than on [`crate::EmbeddedStorage`] directly.
//! Wave-2 swap-ins (the external storage mode at M5) implement the same
//! trait.

use std::path::PathBuf;

use ft_core::{Record, RecordId};

use crate::StorageError;
use crate::filter::StorageFilter;

/// Read/write contract for the JSON-in-Git record store.
///
/// Implementations must be `Send + Sync` so callers can share an
/// `Arc<dyn Storage>` across threads.
pub trait Storage: Send + Sync {
    /// Read a record by ID from the working tree.
    ///
    /// Validates JSON schema and verifies `state_hash` before returning.
    fn read(&self, id: &RecordId) -> Result<Record, StorageError>;

    /// Read a record at a specific git ref.
    ///
    /// Used by history walks and `check pr`. Validates schema and
    /// `state_hash`.
    fn read_at_ref(&self, gitref: &str, id: &RecordId) -> Result<Record, StorageError>;

    /// Write a record atomically.
    ///
    /// Implementations write to `<path>.tmp`, fsync, then rename onto the
    /// final path. Returns the resolved final path.
    ///
    /// Implementations refuse to write a record whose `state_hash` does not
    /// match its recomputed hash and return [`StorageError::HashMismatch`].
    /// Updating `state_hash` before write is the caller's responsibility.
    fn write(&self, record: &Record) -> Result<PathBuf, StorageError>;

    /// Delete a record file from the working tree. Does not stage or commit.
    fn delete(&self, id: &RecordId) -> Result<(), StorageError>;

    /// List record IDs matching `filter`. Order is unspecified.
    ///
    /// `list` returns IDs only; call [`Storage::read`] (or iterate via
    /// [`Storage::iter`]) to fetch bodies.
    fn list(&self, filter: &StorageFilter) -> Result<Vec<RecordId>, StorageError>;

    /// Stream records matching `filter`.
    ///
    /// Each yielded item is a complete validated [`Record`], or the first
    /// error encountered. Used by `ft-index` during full rebuild to avoid
    /// loading every record into memory simultaneously.
    fn iter<'a>(
        &'a self,
        filter: &'a StorageFilter,
    ) -> Box<dyn Iterator<Item = Result<Record, StorageError>> + 'a>;

    /// Pure (no I/O) resolution of the on-disk path that backs `id`.
    fn path_for(&self, id: &RecordId) -> PathBuf;

    /// Root of the records tree (e.g. `<repo>/.firetrail/records/`).
    fn records_root(&self) -> PathBuf;
}
