//! Path → `RecordId` recovery, used to bridge `git diff` output back to typed
//! records.
//!
//! `ft-storage::compact::id_from_record_path` is the canonical implementation
//! but is `pub(crate)` in that crate. We re-derive it here so `ft-pr` does not
//! force its scope to widen.

use ft_core::RecordId;

/// Best-effort recovery of a [`RecordId`] from a record-file path.
///
/// Returns `None` for paths that are not `<lowercase-id>.json` under a known
/// records subdirectory.
#[must_use]
pub(crate) fn id_from_record_path(path: &std::path::Path) -> Option<RecordId> {
    let stem = path.file_stem()?.to_str()?;
    let (prefix, rest) = stem.split_once('-')?;
    let display = format!("{}-{rest}", prefix.to_ascii_uppercase());
    RecordId::from_string(display).ok()
}
