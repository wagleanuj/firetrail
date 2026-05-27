//! Higher-level write helper that appends a history entry before writing.
//!
//! The canonical write path for chain-tracked mutations:
//!
//! 1. The caller mutates the record body (title, status, body fields).
//! 2. The caller hands the record + a [`HistoryDraft`] to
//!    [`write_with_history`].
//! 3. The helper invokes [`ft_history::append_history`] which updates
//!    `state_hash`, `prev_state_hash`, and pushes a new `history[]` entry.
//! 4. The helper writes the record via [`EmbeddedStorage::write`], which
//!    re-checks the embedded `state_hash` against a recomputed canonical
//!    hash before persisting.
//!
//! The pre-existing [`crate::Storage::write`] path remains available for
//! callers that manage history themselves (restore-from-backup, branch
//! salvage, imports) — they take responsibility for the chain invariant.

use ft_core::Record;
use ft_history::{HistoryDraft, HistoryError, append_history};

use crate::StorageError;
use crate::embedded::EmbeddedStorage;
use crate::storage::Storage;

impl From<HistoryError> for StorageError {
    fn from(e: HistoryError) -> Self {
        match e {
            HistoryError::Core(c) => Self::Core(c),
            HistoryError::InvalidDraft(s) => Self::Invalid {
                path: std::path::PathBuf::new(),
                reason: format!("history draft: {s}"),
            },
        }
    }
}

/// Append a history entry to `record`, then persist via [`EmbeddedStorage::write`].
///
/// On success, `record` reflects the post-append state (new `state_hash`,
/// new tail in `history[]`, updated `prev_state_hash`) and the on-disk
/// file matches.
///
/// # Errors
///
/// - Any error from [`ft_history::append_history`] (e.g. zero `ops_count`).
/// - Any error from [`Storage::write`] (e.g. I/O failure).
pub fn write_with_history(
    storage: &EmbeddedStorage,
    record: &mut Record,
    draft: HistoryDraft,
) -> Result<std::path::PathBuf, StorageError> {
    append_history(record, draft)?;
    storage.write(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ft_history::{HistoryEntryKind, verify_chain};
    use ft_testkit::{TestRepo, make_identity, make_task};

    fn draft(kind: HistoryEntryKind, summary: &str) -> HistoryDraft {
        HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc::now(),
            primary_actor: make_identity(),
            contributors: Vec::new(),
            ops_summary: vec![summary.to_string()],
            ops_count: 1,
            kind,
        }
    }

    #[test]
    fn write_with_history_round_trip_verifies() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let mut r = make_task().title("first").build();

        write_with_history(&storage, &mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        assert_eq!(r.envelope.history.len(), 1);

        let back = storage.read(&r.envelope.id).unwrap();
        assert_eq!(back, r);
        verify_chain(&back).expect("chain must verify after write_with_history round trip");
    }

    #[test]
    fn write_with_history_chains_multiple_mutations() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let mut r = make_task().title("v0").build();
        write_with_history(&storage, &mut r, draft(HistoryEntryKind::Create, "born")).unwrap();

        r.envelope.title = "v1".to_string();
        write_with_history(&storage, &mut r, draft(HistoryEntryKind::Update, "rename")).unwrap();
        r.envelope.title = "v2".to_string();
        write_with_history(&storage, &mut r, draft(HistoryEntryKind::Update, "rename2")).unwrap();

        let back = storage.read(&r.envelope.id).unwrap();
        assert_eq!(back.envelope.history.len(), 3);
        verify_chain(&back).expect("chain must verify");
    }

    #[test]
    fn write_with_history_rejects_zero_ops_count() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let mut r = make_task().build();
        let mut d = draft(HistoryEntryKind::Create, "x");
        d.ops_count = 0;
        let err = write_with_history(&storage, &mut r, d).unwrap_err();
        assert!(matches!(err, StorageError::Invalid { .. }));
    }
}
