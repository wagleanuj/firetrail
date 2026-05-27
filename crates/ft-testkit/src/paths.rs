//! On-disk path layout used by ft-testkit when persisting records for
//! assertion helpers.
//!
//! **Conservative mirror of the ft-storage layout** documented in
//! `docs/components/ft-storage.md`:
//!
//! ```text
//! <repo_root>/.firetrail/
//! └── records/
//!     ├── task/<lowercase_id>.json
//!     ├── epic/<lowercase_id>.json
//!     ├── subtask/...
//!     └── bug/...
//! ```
//!
//! Records are written with `<lowercase_id>.json` filenames so the canonical
//! path is reproducible from a `RecordId` alone. ft-storage will eventually
//! own this layout — ft-testkit's helpers will then thin out to delegate
//! through ft-storage.

use std::path::{Path, PathBuf};

use ft_core::{RecordId, RecordKind};

/// Relative path of the records directory under the repo root.
pub const RECORDS_DIR: &str = ".firetrail/records";

/// Subdirectory name for a record kind (lowercase).
#[must_use]
pub fn kind_dir(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Task => "task",
        RecordKind::Epic => "epic",
        RecordKind::Subtask => "subtask",
        RecordKind::Bug => "bug",
        RecordKind::Incident => "incident",
        RecordKind::Finding => "finding",
        RecordKind::Runbook => "runbook",
        RecordKind::Decision => "decision",
        RecordKind::Gotcha => "gotcha",
        RecordKind::Memory => "memory",
    }
}

/// Absolute path to the directory holding records of a given kind, rooted at
/// `repo_root`.
#[must_use]
pub fn records_kind_dir(repo_root: &Path, kind: RecordKind) -> PathBuf {
    repo_root.join(RECORDS_DIR).join(kind_dir(kind))
}

/// Absolute path of the JSON file that backs `id`, rooted at `repo_root`.
#[must_use]
pub fn record_path(repo_root: &Path, id: &RecordId) -> PathBuf {
    records_kind_dir(repo_root, id.kind()).join(format!("{}.json", id.as_str().to_lowercase()))
}
