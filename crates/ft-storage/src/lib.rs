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

mod embedded;
mod error;
mod filter;
mod storage;

pub use embedded::EmbeddedStorage;
pub use error::StorageError;
pub use filter::StorageFilter;
pub use storage::Storage;

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
