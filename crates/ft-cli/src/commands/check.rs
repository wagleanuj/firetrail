//! `firetrail check pr <base> <head>` — pre-commit / pre-merge wrapper around
//! [`ft_storage::validate_pre_commit`].
//!
//! Reports per-path verdicts plus the "memory-only" flag that ADR-0009's
//! relaxed merge gate keys off.

use ft_storage::{ChangeClass, validate_pre_commit};
use serde::Serialize;

use crate::cli::{CheckPrArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "check pr";

/// `firetrail check pr`
pub fn pr(args: &CheckPrArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let git = ft_git::Repo::open(&ctx.ws.root)
        .map_err(|e| CliError::internal(COMMAND, format!("open git: {e}")))?;
    let entries = git
        .diff(&args.base, &args.head, None)
        .map_err(|e| CliError::internal(COMMAND, format!("diff: {e}")))?;
    let paths: Vec<std::path::PathBuf> = entries.into_iter().map(|e| e.path).collect();

    let report = validate_pre_commit(&ctx.storage, &paths);
    let memory_only = report.is_memory_only();
    let clean = report.is_clean();

    let rows: Vec<CheckRow> = report
        .paths
        .iter()
        .map(|p| CheckRow {
            path: p.path.display().to_string(),
            class: class_label(&p.class).to_string(),
            ok: p.failure.is_none(),
            reason: p.failure.clone(),
        })
        .collect();

    let outcome = CheckPrOutcome {
        base: args.base.clone(),
        head: args.head.clone(),
        clean,
        memory_only,
        rows,
        warnings,
    };

    if !clean {
        return Err(CliError::UserError {
            command: COMMAND.into(),
            message: format!(
                "{} record file(s) failed pre-commit validation",
                outcome.rows.iter().filter(|r| !r.ok).count()
            ),
            details: serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null),
        });
    }

    Ok(CommandOutcome::CheckPr(outcome))
}

fn class_label(c: &ChangeClass) -> &'static str {
    match c {
        ChangeClass::Memory(_) => "memory",
        ChangeClass::Structural(_) => "structural",
        ChangeClass::Config => "config",
        ChangeClass::Other => "other",
    }
}

/// Per-path row in the report.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRow {
    /// Path under the repo root.
    pub path: String,
    /// Coarse classification (`memory`, `structural`, `config`, `other`).
    pub class: String,
    /// `true` when validation succeeded (or the path was a no-op classification).
    pub ok: bool,
    /// First failure reason, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Outcome of `firetrail check pr`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckPrOutcome {
    /// Base git ref.
    pub base: String,
    /// Head git ref.
    pub head: String,
    /// `true` iff every record file in the diff verified cleanly.
    pub clean: bool,
    /// `true` iff the diff is composed entirely of memory-kind record files
    /// (ADR-0009 relaxed merge eligibility).
    pub memory_only: bool,
    /// Per-path verdicts.
    pub rows: Vec<CheckRow>,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl CheckPrOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# check pr `{}..{}`\n\nClean: {} · Memory-only: {} · Paths: {}\n",
            self.base,
            self.head,
            self.clean,
            self.memory_only,
            self.rows.len()
        );
        if !self.rows.is_empty() {
            s.push_str("\n| Path | Class | OK | Reason |\n|------|-------|----|--------|\n");
            for r in &self.rows {
                let _ = writeln!(
                    s,
                    "| `{}` | {} | {} | {} |",
                    r.path,
                    r.class,
                    if r.ok { "✓" } else { "✗" },
                    r.reason.as_deref().unwrap_or("—")
                );
            }
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "check pr: clean={} memory_only={} ({} paths)",
            self.clean,
            self.memory_only,
            self.rows.len()
        )
    }
}
