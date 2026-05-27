//! # ft-storage
//!
//! JSON-in-Git read/write storage. The canonical storage layer for Firetrail
//! records. Records are serialized to JSON files under
//! `.firetrail/records/<type>/<id>.json`. Writes are atomic (write `.tmp`
//! then rename). Reads can target any git ref.
//!
//! At M1 only embedded mode is implemented (records co-located with the
//! working repo). External mode (a separate dedicated repository) is added
//! in M5; the [`Storage`] trait is designed so the addition is non-breaking.
//!
//! ## Surface
//!
//! - [`Storage`] — the canonical trait downstream crates depend on
//! - [`EmbeddedStorage`] — the M1 implementation
//! - [`StorageFilter`] — list/iter filter
//! - [`StorageError`] — error variants
//!
//! ## Relevant ADRs
//!
//! - ADR-0002 — JSON-in-Git, not Dolt
//! - ADR-0006 — Storage modes (embedded vs external)
//! - ADR-0011 — Offline-first
//! - ADR-0015 — Hash-based record IDs (lowercase filenames)
//! - ADR-0017 — Audit-chain integrity (`state_hash` verification on write)

mod change;
mod compact;
mod config;
mod embedded;
mod error;
mod external;
mod filter;
mod history;
mod refs;
mod storage;
mod validate;

pub use change::{
    ChangeClass, classify_change, is_memory_kind, is_memory_only_change, records_kind_subpath,
};
pub use compact::{
    CompactRunReport, SkipReason, SkippedPath, compact_changed_in_pr, compact_record,
};
pub use config::{StorageMode, open_for_workspace};
pub use embedded::EmbeddedStorage;
pub use error::StorageError;
pub use external::{
    ExternalConfig, ExternalStorage, SyncPolicy, SyncStatus, ensure_data_repo_cloned, sync_status,
};
pub use filter::StorageFilter;
pub use history::write_with_history;
pub use refs::{ExternalRefViolation, validate_external_references};
pub use storage::Storage;
pub use validate::{
    PathReport, PathStatus, PreCommitReport, validate_pre_commit, validate_pre_commit_diff,
};

/// Relative path of the records directory under the repo root.
pub const RECORDS_DIR: &str = ".firetrail/records";

/// Subdirectory name for a record kind (lowercase).
///
/// Mirrors the layout documented in `docs/components/ft-storage.md`.
#[must_use]
pub fn kind_dir(kind: ft_core::RecordKind) -> &'static str {
    use ft_core::RecordKind;
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
