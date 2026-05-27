//! On-disk path layout used by ft-testkit when persisting records for
//! assertion helpers.
//!
//! These helpers now delegate to `ft-storage` for the canonical layout
//! constants and path resolution. The previous local definitions remain
//! exposed as re-exports so existing test code keeps compiling.

use std::path::{Path, PathBuf};

use ft_core::{RecordId, RecordKind};

/// Relative path of the records directory under the repo root.
///
/// Re-exported from [`ft_storage::RECORDS_DIR`] so there is exactly one
/// source of truth for the on-disk layout.
pub use ft_storage::RECORDS_DIR;

/// Subdirectory name for a record kind (lowercase).
///
/// Re-exported from [`ft_storage::kind_dir`].
pub use ft_storage::kind_dir;

/// Absolute path to the directory holding records of a given kind, rooted at
/// `repo_root`.
#[must_use]
pub fn records_kind_dir(repo_root: &Path, kind: RecordKind) -> PathBuf {
    repo_root.join(RECORDS_DIR).join(kind_dir(kind))
}

/// Absolute path of the JSON file that backs `id`, rooted at `repo_root`.
///
/// Thin wrapper over [`ft_storage::EmbeddedStorage::path_for`] semantics —
/// kept as a free function so tests that have a `&Path` (and not a fully
/// constructed storage handle) can still resolve a record path cheaply.
#[must_use]
pub fn record_path(repo_root: &Path, id: &RecordId) -> PathBuf {
    records_kind_dir(repo_root, id.kind()).join(format!("{}.json", id.as_str().to_lowercase()))
}
