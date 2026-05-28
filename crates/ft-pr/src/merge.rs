//! JSON merge driver for Firetrail record files.
//!
//! Three-way merge of [`Record`] values with custom array-merge rules:
//!
//! - `acceptance_criteria`: union keyed on `text`; status reconciliation
//!   prefers `Checked` over `Unchecked`.
//! - `history`: union keyed on `to_hash`, then sorted by `timestamp`.
//! - Scalar fields: if both sides diverged from `base` to *different* values,
//!   emit a [`Conflict`].
//!
//! [`merge_driver_cli`] is the entry point for `git`'s `%O %A %B` merge driver
//! protocol (`%O` = base, `%A` = ours, `%B` = theirs; the driver writes the
//! merged result to `%A`).

use std::path::{Path, PathBuf};

use ft_core::{AcStatus, AcceptanceCriterion, HistoryEntry, Record, RecordBody, state_hash};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by the merge driver.
#[derive(Debug, Error)]
pub enum MergeError {
    /// Failed to read or write a file backing one of the merge sides.
    #[error("io {path}: {source}")]
    Io {
        /// The file the I/O operation targeted.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to deserialize a side as a Firetrail [`Record`].
    #[error("decode {path}: {source}")]
    Decode {
        /// The file that could not be parsed.
        path: PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Sides disagree on a structural shape change that cannot be
    /// auto-merged (different `kind`, different `id`, …).
    #[error("structural conflict: {0}")]
    Structural(String),

    /// Re-hashing the merged record failed.
    #[error("rehash: {0}")]
    Rehash(#[from] ft_core::CoreError),

    /// Failed to encode the merged record.
    #[error("encode: {0}")]
    Encode(#[from] serde_json::Error),
}

/// A single unresolved divergence between `ours` and `theirs` (where both
/// also differ from `base`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Dotted path of the conflicting field.
    pub field: String,
    /// Value at the common ancestor.
    pub base: serde_json::Value,
    /// Value on the local side.
    pub ours: serde_json::Value,
    /// Value on the incoming side.
    pub theirs: serde_json::Value,
}

/// Result of a three-way merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    /// Merged record. Even when `conflicts` is non-empty, this value is a
    /// best-effort merge so callers can present it in a UI.
    pub merged: Record,
    /// Conflicts that need human resolution.
    pub conflicts: Vec<Conflict>,
}

impl MergeResult {
    /// `true` iff the merge was fully automatic.
    #[must_use]
    pub fn clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// Three-way merge of [`Record`] values.
///
/// `base` is the common ancestor (`None` if the record was added on both
/// sides). `ours` is the local side; `theirs` is the incoming side.
pub fn merge_records(
    base: Option<&Record>,
    ours: &Record,
    theirs: &Record,
) -> Result<MergeResult, MergeError> {
    if ours.envelope.id != theirs.envelope.id {
        return Err(MergeError::Structural(format!(
            "id mismatch: ours={} theirs={}",
            ours.envelope.id.as_str(),
            theirs.envelope.id.as_str()
        )));
    }
    if ours.envelope.kind != theirs.envelope.kind {
        return Err(MergeError::Structural("kind mismatch".to_string()));
    }

    let mut conflicts = Vec::new();
    let mut merged = ours.clone();

    // Envelope scalars merged via base-aware three-way diff.
    merge_scalar(
        "envelope.title",
        base.map(|b| &b.envelope.title),
        &ours.envelope.title,
        &theirs.envelope.title,
        &mut merged.envelope.title,
        &mut conflicts,
    );
    merge_scalar(
        "envelope.status",
        base.map(|b| &b.envelope.status),
        &ours.envelope.status,
        &theirs.envelope.status,
        &mut merged.envelope.status,
        &mut conflicts,
    );
    merge_scalar(
        "envelope.priority",
        base.map(|b| &b.envelope.priority),
        &ours.envelope.priority,
        &theirs.envelope.priority,
        &mut merged.envelope.priority,
        &mut conflicts,
    );
    merge_scalar(
        "envelope.owner",
        base.map(|b| &b.envelope.owner),
        &ours.envelope.owner,
        &theirs.envelope.owner,
        &mut merged.envelope.owner,
        &mut conflicts,
    );
    merge_scalar(
        "envelope.closed_at",
        base.map(|b| &b.envelope.closed_at),
        &ours.envelope.closed_at,
        &theirs.envelope.closed_at,
        &mut merged.envelope.closed_at,
        &mut conflicts,
    );
    merge_scalar(
        "envelope.owning_scope",
        base.map(|b| &b.envelope.owning_scope),
        &ours.envelope.owning_scope,
        &theirs.envelope.owning_scope,
        &mut merged.envelope.owning_scope,
        &mut conflicts,
    );
    // Use the latest of either side for updated_at.
    merged.envelope.updated_at = ours.envelope.updated_at.max(theirs.envelope.updated_at);

    // History: union by to_hash, then sort by timestamp.
    merged.envelope.history = merge_history(
        base.map_or(&[][..], |b| b.envelope.history.as_slice()),
        &ours.envelope.history,
        &theirs.envelope.history,
    );

    // Body acceptance_criteria union (for kinds that have one).
    merge_body_acs(
        base.map(|b| &b.body),
        &ours.body,
        &theirs.body,
        &mut merged.body,
    );

    // Re-stitch the history chain: the union+sort can place entries from
    // both branches at sibling positions whose `from_hash` references the
    // pre-divergence tail rather than the merged predecessor. Re-link so
    // `verify_chain` reads end-to-end. Sets `state_hash` /
    // `prev_state_hash` as a side effect.
    if merged.envelope.history.is_empty() {
        // Recompute state_hash so the merged record is self-consistent.
        merged.envelope.state_hash = String::new();
        merged.envelope.state_hash = state_hash(&merged)?;
    } else {
        ft_history::relink_chain(&mut merged)?;
    }

    Ok(MergeResult { merged, conflicts })
}

fn merge_scalar<T: Clone + PartialEq + Serialize>(
    field: &str,
    base: Option<&T>,
    ours: &T,
    theirs: &T,
    out: &mut T,
    conflicts: &mut Vec<Conflict>,
) {
    if ours == theirs {
        *out = ours.clone();
        return;
    }
    match base {
        Some(b) if b == ours => {
            // Only theirs changed -> take theirs.
            *out = theirs.clone();
        }
        Some(b) if b == theirs => {
            // Only ours changed -> keep ours.
            *out = ours.clone();
        }
        Some(b) => {
            conflicts.push(Conflict {
                field: field.to_string(),
                base: serde_json::to_value(b).unwrap_or(serde_json::Value::Null),
                ours: serde_json::to_value(ours).unwrap_or(serde_json::Value::Null),
                theirs: serde_json::to_value(theirs).unwrap_or(serde_json::Value::Null),
            });
            // Keep ours as the best-effort merged value.
            *out = ours.clone();
        }
        None => {
            // No base. If both added different values, conflict.
            conflicts.push(Conflict {
                field: field.to_string(),
                base: serde_json::Value::Null,
                ours: serde_json::to_value(ours).unwrap_or(serde_json::Value::Null),
                theirs: serde_json::to_value(theirs).unwrap_or(serde_json::Value::Null),
            });
            *out = ours.clone();
        }
    }
}

fn merge_history(
    _base: &[HistoryEntry],
    ours: &[HistoryEntry],
    theirs: &[HistoryEntry],
) -> Vec<HistoryEntry> {
    let mut out: Vec<HistoryEntry> = Vec::with_capacity(ours.len() + theirs.len());
    for h in ours.iter().chain(theirs.iter()) {
        if !out.iter().any(|e| e.to_hash == h.to_hash) {
            out.push(h.clone());
        }
    }
    out.sort_by_key(|h| h.timestamp);
    out
}

fn merge_body_acs(
    base: Option<&RecordBody>,
    ours: &RecordBody,
    theirs: &RecordBody,
    out: &mut RecordBody,
) {
    match (ours, theirs, out) {
        (RecordBody::Task(o), RecordBody::Task(t), RecordBody::Task(m)) => {
            let b = match base {
                Some(RecordBody::Task(b)) => b.acceptance_criteria.as_slice(),
                _ => &[],
            };
            m.acceptance_criteria = merge_acs(b, &o.acceptance_criteria, &t.acceptance_criteria);
        }
        (RecordBody::Subtask(o), RecordBody::Subtask(t), RecordBody::Subtask(m)) => {
            let b = match base {
                Some(RecordBody::Subtask(b)) => b.acceptance_criteria.as_slice(),
                _ => &[],
            };
            m.acceptance_criteria = merge_acs(b, &o.acceptance_criteria, &t.acceptance_criteria);
        }
        (RecordBody::Bug(o), RecordBody::Bug(t), RecordBody::Bug(m)) => {
            let b = match base {
                Some(RecordBody::Bug(b)) => b.acceptance_criteria.as_slice(),
                _ => &[],
            };
            m.acceptance_criteria = merge_acs(b, &o.acceptance_criteria, &t.acceptance_criteria);
        }
        _ => {}
    }
}

fn merge_acs(
    _base: &[AcceptanceCriterion],
    ours: &[AcceptanceCriterion],
    theirs: &[AcceptanceCriterion],
) -> Vec<AcceptanceCriterion> {
    let mut out: Vec<AcceptanceCriterion> = Vec::with_capacity(ours.len() + theirs.len());
    for ac in ours {
        out.push(ac.clone());
    }
    for ac in theirs {
        match out.iter_mut().find(|o| o.text == ac.text) {
            Some(existing) => {
                // Checked wins over Unchecked.
                if existing.status == AcStatus::Unchecked && ac.status == AcStatus::Checked {
                    existing.status = AcStatus::Checked;
                    existing.evidence_url.clone_from(&ac.evidence_url);
                    existing.checked_by.clone_from(&ac.checked_by);
                    existing.checked_at = ac.checked_at;
                }
                existing.updated_at = existing.updated_at.max(ac.updated_at);
            }
            None => out.push(ac.clone()),
        }
    }
    out
}

/// Git merge-driver invocation arguments.
///
/// Maps `%O %A %B` from a `git config merge.<driver>.driver` line.
#[derive(Debug, Clone)]
#[allow(clippy::struct_field_names)]
pub struct MergeDriverArgs {
    /// Path to the common-ancestor blob (`%O`). May be empty/non-existent if
    /// the file was added on both sides.
    pub base_path: PathBuf,
    /// Path to the local side (`%A`). The merged result is written here.
    pub ours_path: PathBuf,
    /// Path to the incoming side (`%B`).
    pub theirs_path: PathBuf,
}

/// Outcome of a [`merge_driver_cli`] invocation. Mirrors `git`'s contract:
/// exit code `0` means clean, non-zero means conflicts remain.
#[derive(Debug, Clone)]
pub struct MergeDriverOutput {
    /// Number of unresolved conflicts.
    pub conflict_count: usize,
    /// Suggested process exit code (0 clean, 1 conflicts).
    pub exit_code: i32,
}

/// Git merge-driver compatible entry point. Reads the three sides from disk,
/// runs [`merge_records`], writes the merged blob to `args.ours_path`.
pub fn merge_driver_cli(args: &MergeDriverArgs) -> Result<MergeDriverOutput, MergeError> {
    let base = read_optional(&args.base_path)?;
    let ours = read_required(&args.ours_path)?;
    let theirs = read_required(&args.theirs_path)?;

    let result = merge_records(base.as_ref(), &ours, &theirs)?;

    let json = serde_json::to_vec_pretty(&result.merged)?;
    std::fs::write(&args.ours_path, json).map_err(|e| MergeError::Io {
        path: args.ours_path.clone(),
        source: e,
    })?;

    let exit_code = i32::from(!result.clean());
    Ok(MergeDriverOutput {
        conflict_count: result.conflicts.len(),
        exit_code,
    })
}

fn read_required(path: &Path) -> Result<Record, MergeError> {
    let bytes = std::fs::read(path).map_err(|e| MergeError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    serde_json::from_slice::<Record>(&bytes).map_err(|e| MergeError::Decode {
        path: path.to_path_buf(),
        source: e,
    })
}

fn read_optional(path: &Path) -> Result<Option<Record>, MergeError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(MergeError::Io {
                path: path.to_path_buf(),
                source: e,
            });
        }
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice::<Record>(&bytes)
        .map(Some)
        .map_err(|e| MergeError::Decode {
            path: path.to_path_buf(),
            source: e,
        })
}
