//! Top-level validation orchestration.
//!
//! [`validate_pr`] walks the diff `base..head`, classifies each change,
//! deserializes the record at both ends where possible, and dispatches every
//! enabled rule. Findings are aggregated into a [`PrReport`].

use std::collections::HashMap;
use std::path::PathBuf;

use ft_core::{Record, RecordId};
use ft_git::{ChangeKind, DiffEntry, Repo};
use ft_storage::{ChangeClass, Storage, classify_change};

use crate::error::PrError;
use crate::options::PrValidatorOptions;
use crate::path::id_from_record_path;
use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::rules;

/// Convenience namespace mirroring [`validate_pr`]. Provided so callers can
/// write `ft_pr::PrValidator::validate_pr(...)` if they prefer.
#[derive(Debug, Default)]
pub struct PrValidator;

impl PrValidator {
    /// Convenience wrapper; see [`validate_pr`].
    pub fn validate_pr(
        storage: &dyn Storage,
        git: &Repo,
        base: &str,
        head: &str,
        opts: &PrValidatorOptions,
    ) -> Result<PrReport, PrError> {
        validate_pr(storage, git, base, head, opts)
    }
}

/// A single record changed by the PR.
///
/// Internal type used by rules to look at base/head views of a record. Rules
/// receive a slice of these plus the raw diff so they can run path-only
/// checks (e.g. mixed-commit) without paying for deserialization.
#[derive(Debug, Clone)]
pub(crate) struct ChangedRecord {
    /// Repo-relative path at head (or the deletion path if the file is gone).
    pub path: PathBuf,
    /// Classification of the path.
    pub class: ChangeClass,
    /// Nature of the change in the diff.
    #[allow(dead_code)]
    pub change_kind: ChangeKind,
    /// Recovered record id from the filename. `None` only if the path does
    /// not parse as a record file (rare; only happens if a non-conforming
    /// file landed under `.firetrail/records/<kind>/`).
    pub id: Option<RecordId>,
    /// Record body at `base`, when the file existed there.
    pub at_base: Option<Record>,
    /// Record body at `head`, when the file exists there.
    pub at_head: Option<Record>,
}

/// Bundle of inputs all rules see.
pub(crate) struct ValidationContext<'a> {
    pub git: &'a Repo,
    pub storage: &'a dyn Storage,
    pub base: &'a str,
    pub head: &'a str,
    pub opts: &'a PrValidatorOptions,
    #[allow(dead_code)]
    pub diff: &'a [DiffEntry],
    pub changed: &'a [ChangedRecord],
    /// Quick lookup of changed records by id (handy for cross-record rules).
    pub by_id: &'a HashMap<RecordId, usize>,
}

/// Walk the diff base..head, classify each change, and dispatch every enabled
/// rule. Returns a structured [`PrReport`].
///
/// Validation never short-circuits on a finding — every rule runs against
/// every relevant record so the report surfaces the full picture in one
/// pass. A [`PrError`] is only returned when validation could not run at all
/// (e.g. one of the refs failed to resolve).
pub fn validate_pr(
    storage: &dyn Storage,
    git: &Repo,
    base: &str,
    head: &str,
    opts: &PrValidatorOptions,
) -> Result<PrReport, PrError> {
    let diff = git.diff(base, head, None)?;

    let all_changed = build_changed_records(storage, git, base, head, &diff)?;

    let mut report = PrReport::default();

    // Partition by pilot-rollout filter (ADR-0021 / scopes.yaml
    // `enabled_scopes`). When the filter is set, records whose
    // `owning_scope` is NOT in the list contribute one Info finding each
    // and are excluded from rule evaluation. Records without an
    // `owning_scope` are always validated — un-scoped infrastructure
    // changes still get the full rule set.
    let changed: Vec<ChangedRecord> = if let Some(list) = opts.enabled_scopes.as_ref() {
        let mut kept = Vec::with_capacity(all_changed.len());
        for c in all_changed {
            if let Some(scope) = changed_record_scope(&c) {
                if !list.iter().any(|s| s == scope) {
                    report.push(PrFinding {
                        severity: Severity::Info,
                        rule: RuleId::ScopeSkipped,
                        record_id: c.id.clone(),
                        path: Some(c.path.clone()),
                        message: format!(
                            "skipped: out of pilot scope `{scope}` (not in enabled_scopes)"
                        ),
                        details: serde_json::json!({
                            "owning_scope": scope,
                            "enabled_scopes": list,
                        }),
                    });
                    continue;
                }
            }
            kept.push(c);
        }
        kept
    } else {
        all_changed
    };

    let by_id: HashMap<RecordId, usize> = changed
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.id.clone().map(|id| (id, i)))
        .collect();

    report.summary.changed_records = changed
        .iter()
        .filter(|c| matches!(c.class, ChangeClass::Memory(_) | ChangeClass::Structural(_)))
        .count();

    let cx = ValidationContext {
        git,
        storage,
        base,
        head,
        opts,
        diff: &diff,
        changed: &changed,
        by_id: &by_id,
    };

    rules::mixed_commit::run(&cx, &mut report);
    rules::chain_broken::run(&cx, &mut report);
    rules::incomplete_acceptance::run(&cx, &mut report);
    rules::evidence_required::run(&cx, &mut report);
    rules::secret_leak::run(&cx, &mut report);
    rules::ac_cap_exceeded::run(&cx, &mut report);
    rules::draft_expired::run(&cx, &mut report);
    rules::deprecated_reference::run(&cx, &mut report);
    rules::pr_link_missing::run(&cx, &mut report);

    if opts.strict && report.summary.warnings > 0 {
        // Promote: keep findings as warnings in the report but flip the
        // bookkeeping so is_clean returns false.
        report.summary.errors += report.summary.warnings;
    }

    Ok(report)
}

fn build_changed_records(
    _storage: &dyn Storage,
    git: &Repo,
    base: &str,
    head: &str,
    diff: &[DiffEntry],
) -> Result<Vec<ChangedRecord>, PrError> {
    let mut out = Vec::with_capacity(diff.len());
    for entry in diff {
        let class = classify_change(&entry.path);
        let id = id_from_record_path(&entry.path);
        let (at_base, at_head) = match (&class, &id) {
            (ChangeClass::Memory(_) | ChangeClass::Structural(_), Some(_)) => (
                read_record_at(git, base, &entry.path),
                read_record_at(git, head, &entry.path),
            ),
            _ => (None, None),
        };
        // Surface decode errors as PrError so callers see catastrophic
        // failures, but tolerate the FileNotInTree case (genuine deletions /
        // additions).
        let at_base = match at_base {
            Some(Ok(r)) => Some(r),
            Some(Err(PrError::Git(ft_git::GitError::FileNotInTree(_, _)))) | None => None,
            Some(Err(e)) => return Err(e),
        };
        let at_head = match at_head {
            Some(Ok(r)) => Some(r),
            Some(Err(PrError::Git(ft_git::GitError::FileNotInTree(_, _)))) | None => None,
            Some(Err(e)) => return Err(e),
        };

        out.push(ChangedRecord {
            path: entry.path.clone(),
            class,
            change_kind: entry.change_kind.clone(),
            id,
            at_base,
            at_head,
        });
    }
    Ok(out)
}

/// Return the `owning_scope` for a changed record, preferring the head view
/// (post-change) and falling back to the base view. Records that exist on
/// neither side (e.g. non-record diff entries) and records with no owning
/// scope return `None`.
fn changed_record_scope(c: &ChangedRecord) -> Option<&str> {
    c.at_head
        .as_ref()
        .or(c.at_base.as_ref())
        .and_then(|r| r.envelope.owning_scope.as_deref())
}

fn read_record_at(
    git: &Repo,
    gitref: &str,
    path: &std::path::Path,
) -> Option<Result<Record, PrError>> {
    match git.read_file_at_ref(gitref, path) {
        Ok(bytes) => Some(decode(path, &bytes)),
        Err(ft_git::GitError::FileNotInTree(_, _)) => None,
        Err(e) => Some(Err(PrError::Git(e))),
    }
}

fn decode(path: &std::path::Path, bytes: &[u8]) -> Result<Record, PrError> {
    serde_json::from_slice::<Record>(bytes).map_err(|e| PrError::Decode {
        path: path.display().to_string(),
        source: e,
    })
}
