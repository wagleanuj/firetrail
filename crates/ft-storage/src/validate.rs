//! Pre-commit validation helpers.
//!
//! Wraps [`classify_change`] + [`ft_history::verify_chain`] + on-disk
//! state-hash recomputation into a single report the commit hook can
//! print and act on (ADR-0017).

use std::path::{Path, PathBuf};

use ft_history::verify_chain;

use crate::change::{ChangeClass, classify_change};
use crate::embedded::EmbeddedStorage;
use crate::storage::Storage;

/// Per-path verdict status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStatus {
    /// Path is not a record file (Config or Other) — pass-through.
    Skipped,
    /// Record file loaded and chain verified cleanly.
    Clean,
    /// Record path the caller signalled as deleted in this commit.
    /// Legitimate redaction or removal — not a failure (firetrail-ff0).
    Deleted,
    /// Record file failed to load or failed [`verify_chain`].
    Failure(String),
}

/// Per-path verdict produced by [`validate_pre_commit`] /
/// [`validate_pre_commit_diff`].
#[derive(Debug, Clone)]
pub struct PathReport {
    /// Path the verdict applies to (echoed from the input slice).
    pub path: PathBuf,
    /// Coarse classification of the path.
    pub class: ChangeClass,
    /// Verdict status for this path.
    pub status: PathStatus,
}

impl PathReport {
    /// `true` iff the path failed to load or verify.
    /// Deletions and skipped paths are not failures.
    #[must_use]
    pub fn is_failure(&self) -> bool {
        matches!(self.status, PathStatus::Failure(_))
    }

    /// Failure reason if this report is a [`PathStatus::Failure`], else `None`.
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        match &self.status {
            PathStatus::Failure(r) => Some(r.as_str()),
            _ => None,
        }
    }

    /// `true` iff this path was signalled as a deletion.
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        matches!(self.status, PathStatus::Deleted)
    }
}

/// Aggregate report for [`validate_pre_commit`].
#[derive(Debug, Clone, Default)]
pub struct PreCommitReport {
    /// Per-path verdicts in input order.
    pub paths: Vec<PathReport>,
}

impl PreCommitReport {
    /// `true` iff every input path that classified as a record file
    /// loaded and verified cleanly. Config/Other paths are ignored for
    /// this check — they cannot break chain integrity.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.paths.iter().all(|p| !p.is_failure())
    }

    /// `true` iff every input path is a memory-kind record file (per
    /// [`ChangeClass::is_memory`]). Empty input returns `false`.
    #[must_use]
    pub fn is_memory_only(&self) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        self.paths.iter().all(|p| p.class.is_memory())
    }

    /// Iterator over failed paths.
    pub fn failures(&self) -> impl Iterator<Item = &PathReport> {
        self.paths.iter().filter(|p| p.is_failure())
    }
}

/// Validate `changed_paths` against `storage` before allowing a commit.
///
/// For each path:
///
/// 1. Classify it via [`classify_change`].
/// 2. If it's a record file (Memory or Structural), read it through
///    [`Storage::read`] (which deserializes, schema-validates, and
///    re-computes the embedded `state_hash`).
/// 3. If the read succeeds, run [`verify_chain`] to catch in-record
///    chain breaks.
///
/// Failures are aggregated rather than short-circuited so the report
/// surfaces every offender in one pass.
///
/// Paths that classify as Config or Other are not opened — they cannot
/// participate in the chain invariant and are pass-through.
///
/// Paths whose record file does not exist on disk are reported as a
/// failure ("missing"). Use [`validate_pre_commit_diff`] to signal
/// legitimate deletions and avoid that false-positive (firetrail-ff0).
#[must_use]
pub fn validate_pre_commit<P: AsRef<Path>>(
    storage: &EmbeddedStorage,
    changed_paths: &[P],
) -> PreCommitReport {
    validate_pre_commit_diff::<P, &Path>(storage, changed_paths, &[])
}

/// Diff-aware variant of [`validate_pre_commit`].
///
/// `changed_paths` are added/modified record files. `deleted_paths` are
/// record files removed in this commit — they are surfaced as
/// [`PathStatus::Deleted`] reports and do not trip [`PreCommitReport::is_clean`].
///
/// Non-record deletions (Config / Other) are silently dropped: nothing in
/// `validate_pre_commit`'s contract applies to them, and the chain integrity
/// check is not relevant.
#[must_use]
pub fn validate_pre_commit_diff<P, D>(
    storage: &EmbeddedStorage,
    changed_paths: &[P],
    deleted_paths: &[D],
) -> PreCommitReport
where
    P: AsRef<Path>,
    D: AsRef<Path>,
{
    let mut out = PreCommitReport::default();
    for p in changed_paths {
        let path = p.as_ref().to_path_buf();
        let class = classify_change(&path);

        let status = match &class {
            ChangeClass::Memory(_) | ChangeClass::Structural(_) => {
                match check_record_file(storage, &path) {
                    None => PathStatus::Clean,
                    Some(reason) => PathStatus::Failure(reason),
                }
            }
            ChangeClass::Config | ChangeClass::Other => PathStatus::Skipped,
        };

        out.paths.push(PathReport {
            path,
            class,
            status,
        });
    }
    for p in deleted_paths {
        let path = p.as_ref().to_path_buf();
        let class = classify_change(&path);
        // Only surface record-file deletions — non-record deletions are
        // irrelevant to chain integrity and would clutter the report.
        if !matches!(class, ChangeClass::Memory(_) | ChangeClass::Structural(_)) {
            continue;
        }
        out.paths.push(PathReport {
            path,
            class,
            status: PathStatus::Deleted,
        });
    }
    out
}

/// Read a record file via [`Storage::read`] and run [`verify_chain`].
/// Returns `None` on success, `Some(reason)` on the first failure.
fn check_record_file(storage: &EmbeddedStorage, path: &Path) -> Option<String> {
    // Recover the canonical id from the filename, then funnel through
    // Storage::read so the same parse/schema/hash gates fire.
    let Some(id) = crate::compact::id_from_record_path(path) else {
        return Some(format!("not a record file: {}", path.display()));
    };

    let record = match storage.read(&id) {
        Ok(r) => r,
        Err(e) => return Some(format!("read failed: {e}")),
    };

    match verify_chain(&record) {
        Ok(()) => None,
        Err(e) => Some(format!("chain: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
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
            transition: None,
        }
    }

    fn open(tr: &TestRepo) -> EmbeddedStorage {
        EmbeddedStorage::open(tr.root()).unwrap()
    }

    #[test]
    fn validate_clean_record_returns_clean() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        let mut r = make_task().title("clean").build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        let path = storage.write(&r).unwrap();
        // Use the repo-relative form, which is what `git diff` would give.
        let rel = path.strip_prefix(tr.root()).unwrap().to_path_buf();

        let report = validate_pre_commit(&storage, &[rel]);
        assert!(
            report.is_clean(),
            "{:?}",
            report.failures().collect::<Vec<_>>()
        );
        assert!(!report.is_memory_only()); // task is structural
    }

    #[test]
    fn validate_detects_tampered_state_hash_on_disk() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        let mut r = make_task().title("orig").build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        let path = storage.write(&r).unwrap();

        // Tamper directly on disk: rewrite the title bytes without
        // updating state_hash. ft-storage's read path catches this as a
        // HashMismatch which validate_pre_commit surfaces in its report.
        let bytes = std::fs::read(&path).unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["envelope"]["title"] = serde_json::json!("tampered");
        std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

        let rel = path.strip_prefix(tr.root()).unwrap().to_path_buf();
        let report = validate_pre_commit(&storage, &[rel]);
        assert!(!report.is_clean());
        let failures: Vec<_> = report.failures().collect();
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0]
                .failure()
                .is_some_and(|r| r.contains("read failed") || r.contains("hash")),
            "got: {:?}",
            failures[0].failure()
        );
    }

    #[test]
    fn validate_pre_commit_diff_treats_deletion_as_non_failure() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        // Synthesize a deleted record-file path — no need to write it
        // first, the diff signal is the source of truth.
        let deleted: std::path::PathBuf = ".firetrail/records/task/task-abc.json".into();

        let changed: [std::path::PathBuf; 0] = [];
        let report = validate_pre_commit_diff(&storage, &changed, std::slice::from_ref(&deleted));
        assert!(report.is_clean(), "deletion must not be a failure");
        assert_eq!(report.paths.len(), 1);
        assert!(report.paths[0].is_deleted());
        assert_eq!(report.paths[0].path, deleted);
    }

    #[test]
    fn validate_pre_commit_diff_drops_non_record_deletions() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        let deleted = [
            std::path::PathBuf::from("README.md"),
            std::path::PathBuf::from(".firetrail/config.yml"),
        ];
        let changed: [std::path::PathBuf; 0] = [];
        let report = validate_pre_commit_diff(&storage, &changed, &deleted);
        // Non-record deletions are not surfaced (they cannot affect chain
        // integrity); the report is empty and trivially clean.
        assert!(report.paths.is_empty());
    }

    #[test]
    fn validate_passes_through_config_and_other_paths() {
        let tr = TestRepo::new().unwrap();
        let storage = open(&tr);
        let paths = [
            std::path::PathBuf::from("src/main.rs"),
            std::path::PathBuf::from(".firetrail/scope.yaml"),
        ];
        let report = validate_pre_commit(&storage, &paths);
        assert!(report.is_clean());
        assert!(!report.is_memory_only());
    }
}
