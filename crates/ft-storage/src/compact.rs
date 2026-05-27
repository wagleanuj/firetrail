//! Compaction helpers that bridge [`ft_history::compact_history`] to the
//! on-disk storage layer (ADR-0003).
//!
//! At PR-time the merge gate runs [`compact_changed_in_pr`] over every
//! record file touched between the PR base and head refs, calling
//! [`ft_history::compact_history`] on each and writing the compacted
//! result back. The chain remains verifiable end-to-end.

use ft_core::RecordId;
use ft_history::{CompactPolicy, CompactReport, compact_history};

use crate::change::classify_change;
use crate::embedded::EmbeddedStorage;
use crate::storage::Storage;
use crate::{ChangeClass, StorageError};

/// Compact a single record's history and write it back.
///
/// Reads the record from disk, runs [`compact_history`] under `policy`,
/// then persists the compacted record via [`Storage::write`] (which
/// re-validates the freshly computed `state_hash`).
///
/// Returns the [`CompactReport`] produced by the underlying compaction —
/// callers typically log it to surface how much was squashed.
///
/// # Errors
///
/// - [`StorageError::NotFound`] if `id` is not present on disk.
/// - [`StorageError`] variants from read/write.
/// - [`StorageError::Core`] if the canonical hash recomputation fails.
pub fn compact_record(
    storage: &EmbeddedStorage,
    id: &RecordId,
    policy: &CompactPolicy,
) -> Result<CompactReport, StorageError> {
    let mut record = storage.read(id)?;
    let report = compact_history(&mut record, policy).map_err(StorageError::Core)?;
    storage.write(&record)?;
    Ok(report)
}

/// Compact every record file changed between two git refs.
///
/// Walks `git.diff(pr_base_ref, pr_head_ref, None)`, filters to record
/// files (see [`classify_change`] — both `Memory` and `Structural`
/// kinds are included; `Config` and `Other` paths are skipped), and
/// calls [`compact_record`] on each.
///
/// Returns one entry per record that was successfully compacted.
/// Records that have been deleted in the head ref are skipped silently
/// — there is nothing to compact. Records whose file cannot be parsed
/// (e.g. malformed JSON in mid-PR state) bubble up as a [`StorageError`]
/// and short-circuit the run.
///
/// # Errors
///
/// - [`StorageError::Git`] if the diff itself fails (e.g. a bad ref).
/// - Any error from [`compact_record`] — short-circuits the loop.
pub fn compact_changed_in_pr(
    storage: &EmbeddedStorage,
    git: &ft_git::Repo,
    pr_base_ref: &str,
    pr_head_ref: &str,
    policy: &CompactPolicy,
) -> Result<Vec<(RecordId, CompactReport)>, StorageError> {
    let entries = git
        .diff(pr_base_ref, pr_head_ref, None)
        .map_err(StorageError::Git)?;

    let mut out = Vec::new();
    for entry in entries {
        // We only care about live record paths in the head ref.
        // Deletes have no surviving record to compact; renames are
        // followed via the new path which is what `entry.path` holds.
        if matches!(entry.change_kind, ft_git::ChangeKind::Deleted) {
            continue;
        }
        let class = classify_change(&entry.path);
        if !matches!(class, ChangeClass::Memory(_) | ChangeClass::Structural(_)) {
            continue;
        }

        // Recover the record id from the file's basename
        // (`<lowercase-id>.json`). We do this rather than reading the
        // file twice (once to learn the id, once via storage.read).
        let Some(id) = id_from_record_path(&entry.path) else {
            continue;
        };

        // Skip records whose file no longer exists in the working tree
        // (e.g. the diff was computed from refs but the working tree is
        // mid-checkout). compact_record would surface NotFound; we treat
        // it as a benign skip.
        let on_disk = storage.path_for(&id);
        if !on_disk.exists() {
            continue;
        }

        let report = compact_record(storage, &id, policy)?;
        out.push((id, report));
    }
    Ok(out)
}

/// Best-effort recovery of a [`RecordId`] from a record-file path.
///
/// `EmbeddedStorage` writes records to `<lowercase-id>.json`. We rebuild
/// the canonical id by reading the filename stem and parsing it through
/// the public [`ft_core::RecordId`] deserializer. Returns `None` on any
/// shape mismatch — callers treat that as "not a record file".
pub(crate) fn id_from_record_path(path: &std::path::Path) -> Option<RecordId> {
    let stem = path.file_stem()?.to_str()?;
    // `RecordId` deserializes from a transparent String; canonical form
    // has the uppercase kind prefix. The on-disk filename uses the
    // lowercase form, so we must split on '-' and uppercase the prefix.
    let (prefix, rest) = stem.split_once('-')?;
    let display = format!("{}-{rest}", prefix.to_ascii_uppercase());
    serde_json::from_value::<RecordId>(serde_json::Value::String(display)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
    use ft_testkit::{TestRepo, make_identity, make_task};

    fn draft_at(kind: HistoryEntryKind, summary: &str, secs: i64) -> HistoryDraft {
        HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc.timestamp_opt(secs, 0).single().unwrap(),
            primary_actor: make_identity(),
            contributors: Vec::new(),
            ops_summary: vec![summary.to_string()],
            ops_count: 1,
            kind,
        }
    }

    fn open(tr: &TestRepo) -> EmbeddedStorage {
        EmbeddedStorage::open(tr.root()).unwrap()
    }

    #[test]
    fn compact_record_reduces_history_and_persists() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        let mut r = make_task().title("v0").build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        // 4 same-author updates within an hour → squashable.
        for i in 0..4 {
            r.envelope.title = format!("v{i}");
            append_history(
                &mut r,
                draft_at(HistoryEntryKind::Update, &format!("u{i}"), 60 + i * 60),
            )
            .unwrap();
        }
        storage.write(&r).unwrap();
        let before_len = r.envelope.history.len();
        assert_eq!(before_len, 5);

        let policy = CompactPolicy {
            squash_updates_within: Duration::hours(1),
            ..CompactPolicy::default()
        };
        let report = compact_record(&storage, &r.envelope.id, &policy).unwrap();
        assert_eq!(report.entries_before, 5);
        // Create + one squashed Update = 2.
        assert_eq!(report.entries_after, 2);
        assert!(report.entries_after < report.entries_before);

        // Round-trip: read back, chain still verifies.
        let back = storage.read(&r.envelope.id).unwrap();
        assert_eq!(back.envelope.history.len(), 2);
        ft_history::verify_chain(&back).expect("chain still verifies after compact");
    }

    #[test]
    fn compact_record_missing_id_is_not_found() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        let r = make_task().build();
        let err = compact_record(&storage, &r.envelope.id, &CompactPolicy::default()).unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[test]
    fn id_from_record_path_round_trip() {
        let r = make_task().build();
        let path = std::path::PathBuf::from(".firetrail/records/task")
            .join(format!("{}.json", r.envelope.id.as_str().to_lowercase()));
        let recovered = id_from_record_path(&path).unwrap();
        assert_eq!(recovered, r.envelope.id);
    }

    #[test]
    fn compact_changed_in_pr_walks_diff() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);

        // TestRepo::new already plants an initial empty commit on `main`.
        // Branch off, write a record with squashable history, commit it.
        tr.branch("feat").unwrap();
        tr.checkout("feat").unwrap();
        let mut r = make_task().title("v0").build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        for i in 0..3 {
            r.envelope.title = format!("v{i}");
            append_history(
                &mut r,
                draft_at(HistoryEntryKind::Update, &format!("u{i}"), 60 + i * 60),
            )
            .unwrap();
        }
        storage.write(&r).unwrap();
        tr.commit_all("add record on feat").unwrap();

        let git = ft_git::Repo::open(tr.root()).unwrap();
        let reports =
            compact_changed_in_pr(&storage, &git, "main", "feat", &CompactPolicy::default())
                .unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].0, r.envelope.id);
        assert!(reports[0].1.entries_after < reports[0].1.entries_before);
    }
}
