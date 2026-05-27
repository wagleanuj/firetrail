//! `firetrail compact [<id> | --pr <base..head>]` — PR-time history compaction.
//!
//! Wraps [`ft_storage::compact_record`] for single-record compaction and
//! [`ft_storage::compact_changed_in_pr`] for sweeping a PR diff.

use ft_history::CompactPolicy;
use ft_storage::{compact_changed_in_pr, compact_record};
use serde::Serialize;

use crate::cli::{CompactArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "compact";

/// `firetrail compact`
pub fn run(args: &CompactArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let policy = CompactPolicy::default();

    let reports: Vec<CompactRow> = match (&args.id, &args.pr) {
        (Some(raw), None) => {
            let id = ctx.resolve_id(raw)?;
            let report = compact_record(&ctx.storage, &id, &policy)
                .map_err(|e| CliError::internal(COMMAND, format!("compact: {e}")))?;
            // Refresh the index for the touched path.
            let path = ctx.storage_path_for(&id);
            ctx.index
                .refresh(&ctx.storage, std::slice::from_ref(&path), &[])
                .map_err(|e| CliError::internal(COMMAND, format!("refresh: {e}")))?;
            vec![CompactRow::from_report(id.as_str(), &report)]
        }
        (None, Some(range)) => {
            let (base, head) = parse_range(range)?;
            let git = ft_git::Repo::open(&ctx.ws.root)
                .map_err(|e| CliError::internal(COMMAND, format!("open git: {e}")))?;
            let raw = compact_changed_in_pr(&ctx.storage, &git, &base, &head, &policy)
                .map_err(|e| CliError::internal(COMMAND, format!("compact pr: {e}")))?;
            raw.into_iter()
                .map(|(id, rep)| CompactRow::from_report(id.as_str(), &rep))
                .collect()
        }
        (None, None) => {
            return Err(CliError::user(
                COMMAND,
                "supply either <id> or --pr <base..head>",
            ));
        }
        (Some(_), Some(_)) => {
            return Err(CliError::user(COMMAND, "supply only one of <id> or --pr"));
        }
    };

    Ok(CommandOutcome::Compact(CompactOutcome {
        reports,
        warnings,
    }))
}

fn parse_range(range: &str) -> Result<(String, String), CliError> {
    range
        .split_once("..")
        .map(|(b, h)| (b.to_string(), h.to_string()))
        .filter(|(b, h)| !b.is_empty() && !h.is_empty())
        .ok_or_else(|| CliError::user(COMMAND, format!("--pr must be `base..head`, got `{range}`")))
}

/// Per-record compaction row.
#[derive(Debug, Clone, Serialize)]
pub struct CompactRow {
    /// Canonical record id.
    pub id: String,
    /// History length before compaction.
    pub entries_before: usize,
    /// History length after compaction.
    pub entries_after: usize,
    /// Number of entries dropped.
    pub dropped: usize,
}

impl CompactRow {
    fn from_report(id: &str, r: &ft_history::CompactReport) -> Self {
        Self {
            id: id.to_string(),
            entries_before: r.entries_before,
            entries_after: r.entries_after,
            dropped: r.dropped.len(),
        }
    }
}

/// Outcome of a `compact` invocation.
#[derive(Debug, Clone, Serialize)]
pub struct CompactOutcome {
    /// Per-record compaction summaries.
    pub reports: Vec<CompactRow>,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl CompactOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        if self.reports.is_empty() {
            return "_no records compacted_\n".to_string();
        }
        let mut s =
            String::from("| ID | Before | After | Dropped |\n|----|--------|-------|---------|\n");
        for r in &self.reports {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} |",
                r.id, r.entries_before, r.entries_after, r.dropped
            );
        }
        s
    }

    /// One-line summary for `--quiet`.
    pub fn quiet_line(&self) -> String {
        format!("compact: {} record(s) compacted", self.reports.len())
    }
}

// Tiny helper extension so the WorkCtx can hand out the on-disk path for an
// id without exposing private fields. We keep it inline to avoid editing
// the context module.
impl WorkCtx {
    /// Resolve the absolute on-disk path for a record id without I/O.
    pub fn storage_path_for(&self, id: &ft_core::RecordId) -> std::path::PathBuf {
        ft_storage::Storage::path_for(&self.storage, id)
    }
}
