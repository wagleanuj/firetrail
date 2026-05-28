//! `diff` op — record-aware diff between two git refs.
//!
//! Mirrors `ft_cli::commands::diff`. Each row carries the resolved record id,
//! kind, change classification, owning scope, and head-side title — enough to
//! render a side-by-side viewer without re-reading the records.

use ft_core::Record;
use ft_git::{ChangeKind, Repo};
use ft_storage::{ChangeClass, classify_change};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

/// State-change classification in the diff report.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiffChange {
    /// Record file did not exist at base.
    Created,
    /// Record file exists at both refs.
    Modified,
    /// Record file existed at base but not at head.
    Removed,
    /// Record file was renamed.
    Renamed,
}

/// One row of the diff report.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffRow {
    /// Repo-relative path.
    pub path: String,
    /// Resolved record id, if classifiable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Record kind (lowercase).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Coarse classification (`memory` / `structural` / `config` / `other`).
    pub class: String,
    /// State change.
    pub change: DiffChange,
    /// Owning scope, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// One-line title at head (or base, on deletion), when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Input for [`diff`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "DiffInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffInput {
    /// Base git ref.
    pub base: String,
    /// Head git ref.
    pub head: String,
    /// Restrict to memory-kind changes.
    #[serde(default)]
    pub memory_only: bool,
    /// Scope prefix filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`diff`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "DiffOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffOutput {
    /// Base ref echoed back.
    pub base: String,
    /// Head ref echoed back.
    pub head: String,
    /// Whether the `memory_only` filter was on.
    pub memory_only_filter: bool,
    /// Scope filter prefix, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_filter: Option<String>,
    /// Per-record rows.
    pub rows: Vec<DiffRow>,
}

/// `diff` op.
#[allow(clippy::needless_pass_by_value)]
pub fn diff(
    ws: &Workspace,
    _identity: &Identity,
    input: DiffInput,
    _events: &EventBus,
) -> Result<DiffOutput, OpsError> {
    let git = Repo::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open git: {e}")))?;
    let entries = git
        .diff(&input.base, &input.head, None)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("diff: {e}")))?;

    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let abs = ws.root.join(&entry.path);
        if abs.is_dir() {
            continue;
        }
        let class = classify_change(&entry.path);
        let (is_record, kind) = match &class {
            ChangeClass::Memory(k) | ChangeClass::Structural(k) => (true, Some(*k)),
            _ => (false, None),
        };

        let change = match &entry.change_kind {
            ChangeKind::Added => DiffChange::Created,
            ChangeKind::Deleted => DiffChange::Removed,
            ChangeKind::Modified => DiffChange::Modified,
            ChangeKind::Renamed { .. } => DiffChange::Renamed,
        };

        let mut row = DiffRow {
            path: entry.path.display().to_string(),
            id: None,
            kind: kind.map(|k| format!("{k:?}").to_ascii_lowercase()),
            class: class_label(&class).to_string(),
            change,
            scope: None,
            title: None,
        };

        if is_record {
            let head_ref: &str = if change == DiffChange::Removed {
                input.base.as_str()
            } else {
                input.head.as_str()
            };
            if let Some(record) = read_record_at(&git, head_ref, &entry.path) {
                row.id = Some(record.envelope.id.as_str().to_string());
                row.scope.clone_from(&record.envelope.owning_scope);
                row.title = Some(record.envelope.title.clone());
            }
        }

        if input.memory_only && !matches!(class, ChangeClass::Memory(_)) {
            continue;
        }
        if let Some(want) = &input.scope {
            match &row.scope {
                Some(s) if s.starts_with(want) => {}
                _ => continue,
            }
        }
        rows.push(row);
    }

    Ok(DiffOutput {
        base: input.base,
        head: input.head,
        memory_only_filter: input.memory_only,
        scope_filter: input.scope,
        rows,
    })
}

fn class_label(c: &ChangeClass) -> &'static str {
    match c {
        ChangeClass::Memory(_) => "memory",
        ChangeClass::Structural(_) => "structural",
        ChangeClass::Config => "config",
        ChangeClass::Other => "other",
    }
}

fn read_record_at(git: &Repo, gitref: &str, path: &std::path::Path) -> Option<Record> {
    let bytes = git.read_file_at_ref(gitref, path).ok()?;
    serde_json::from_slice::<Record>(&bytes).ok()
}
