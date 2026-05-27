//! `firetrail diff <base> <head>` — record-aware diff between two git refs.
//!
//! Walks `git diff` for changed paths, classifies each via
//! [`ft_storage::classify_change`], and produces a per-record summary
//! (created / modified / removed). `--memory` restricts the listing to
//! memory-kind records; `--scope` filters by `owning_scope` prefix.

use ft_core::Record;
use ft_git::{ChangeKind, Repo};
use ft_storage::{ChangeClass, classify_change};
use serde::Serialize;

use crate::cli::{DiffArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "diff";

/// State-change classification used in the diff report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StateChange {
    /// Record file did not exist at base.
    Created,
    /// Record file exists at both base and head.
    Modified,
    /// Record file existed at base but not at head.
    Removed,
    /// Record file renamed.
    Renamed,
}

/// One row of the diff report.
#[derive(Debug, Clone, Serialize)]
pub struct DiffRow {
    /// Repo-relative record path.
    pub path: String,
    /// Resolved record id (lowercase form), if recoverable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Record kind, when classifiable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Coarse classification.
    pub class: String,
    /// State change summary.
    pub change: StateChange,
    /// Owning scope if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// One-line title at head (when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// `firetrail diff`
pub fn run(args: &DiffArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let git = Repo::open(&ctx.ws.root)
        .map_err(|e| CliError::internal(COMMAND, format!("open git: {e}")))?;
    let entries = git
        .diff(&args.base, &args.head, None)
        .map_err(|e| CliError::internal(COMMAND, format!("diff: {e}")))?;

    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let class = classify_change(&entry.path);
        let (is_record, kind) = match &class {
            ChangeClass::Memory(k) | ChangeClass::Structural(k) => (true, Some(*k)),
            _ => (false, None),
        };

        let change = match &entry.change_kind {
            ChangeKind::Added => StateChange::Created,
            ChangeKind::Deleted => StateChange::Removed,
            ChangeKind::Modified => StateChange::Modified,
            ChangeKind::Renamed { .. } => StateChange::Renamed,
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
            // Best-effort: pull the title and scope from head (or base on deletion).
            let head_ref: &str = if change == StateChange::Removed {
                args.base.as_str()
            } else {
                args.head.as_str()
            };
            if let Some(record) = read_record_at(&git, head_ref, &entry.path) {
                row.id = Some(record.envelope.id.as_str().to_string());
                row.scope.clone_from(&record.envelope.owning_scope);
                row.title = Some(record.envelope.title.clone());
            }
        }

        // Apply filters.
        if args.memory && !matches!(class, ChangeClass::Memory(_)) {
            continue;
        }
        if let Some(want) = &args.scope {
            match &row.scope {
                Some(s) if s.starts_with(want) => {}
                _ => continue,
            }
        }

        rows.push(row);
    }

    Ok(CommandOutcome::Diff(DiffOutcome {
        base: args.base.clone(),
        head: args.head.clone(),
        memory_only_filter: args.memory,
        scope_filter: args.scope.clone(),
        rows,
        warnings,
    }))
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

/// Outcome of `firetrail diff`.
#[derive(Debug, Clone, Serialize)]
pub struct DiffOutcome {
    /// Base git ref.
    pub base: String,
    /// Head git ref.
    pub head: String,
    /// Whether `--memory` was set.
    pub memory_only_filter: bool,
    /// Scope filter prefix, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_filter: Option<String>,
    /// Per-record rows.
    pub rows: Vec<DiffRow>,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl DiffOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# diff `{}..{}`\n\n{} changed record(s){}\n",
            self.base,
            self.head,
            self.rows.len(),
            if self.memory_only_filter {
                " (memory-only)"
            } else {
                ""
            }
        );
        if !self.rows.is_empty() {
            s.push_str("\n| Change | Kind | ID | Title | Path |\n|---|---|---|---|---|\n");
            for r in &self.rows {
                let _ = writeln!(
                    s,
                    "| {:?} | {} | `{}` | {} | `{}` |",
                    r.change,
                    r.kind.as_deref().unwrap_or("—"),
                    r.id.as_deref().unwrap_or("—"),
                    r.title.as_deref().unwrap_or("—"),
                    r.path,
                );
            }
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!("diff: {} changed record(s)", self.rows.len())
    }
}
