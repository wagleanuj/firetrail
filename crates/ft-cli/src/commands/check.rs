//! `firetrail check pr <base> <head>` — full ft-pr validation pass.
//! `firetrail check paths <paths>…` — per-commit validator (ft-storage).
//!
//! `check pr` runs the complete [`ft_pr::validate_pr`] rule set against the
//! diff between two git refs and surfaces a structured [`ft_pr::PrReport`].
//! `check paths` retains the M2 per-commit semantics (path-list validation
//! against `state_hash` and chain integrity).

use ft_pr::{PrReport, PrValidatorOptions, default_secret_patterns, validate_pr};
use ft_scope::ScopeRegistry;
use ft_storage::{ChangeClass, validate_pre_commit};
use serde::Serialize;

use crate::cli::{CheckPathsArgs, CheckPrArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND_PR: &str = "check pr";
const COMMAND_PATHS: &str = "check paths";

/// `firetrail check pr` — full ft-pr validation.
pub fn pr(args: &CheckPrArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_PR, global.workspace.as_deref())?;

    // Load scopes.yaml (best-effort): a parse failure becomes a warning and
    // we proceed with no pilot filter. A missing file is fine and yields an
    // empty registry whose `enabled_scopes_list` is `None`, matching the
    // legacy "validate every record" surface.
    let enabled_scopes = match ScopeRegistry::load(&ctx.ws.root) {
        Ok(reg) => reg.enabled_scopes_list().map(<[String]>::to_vec),
        Err(e) => {
            ctx.warnings.push(format!(
                "scope registry unavailable: {e}; ignoring pilot filter"
            ));
            None
        }
    };
    let warnings = ctx.warnings.clone();

    let git = ft_git::Repo::open(&ctx.ws.root)
        .map_err(|e| CliError::internal(COMMAND_PR, format!("open git: {e}")))?;

    let mut opts = PrValidatorOptions {
        strict: args.strict,
        enable_secret_scan: !args.no_secret_scan,
        enabled_scopes,
        ..PrValidatorOptions::default()
    };
    // Keep secret patterns in sync with the toggle so JSON consumers see a
    // sensible payload when the scan is disabled.
    if opts.enable_secret_scan {
        opts.secret_patterns = default_secret_patterns();
    } else {
        opts.secret_patterns = Vec::new();
    }

    let report = validate_pr(&ctx.storage, &git, &args.base, &args.head, &opts)
        .map_err(|e| CliError::internal(COMMAND_PR, format!("validate: {e}")))?;

    let clean = report.is_clean();
    let outcome = CheckPrOutcome {
        base: args.base.clone(),
        head: args.head.clone(),
        strict: args.strict,
        secret_scan_enabled: opts.enable_secret_scan,
        report,
        warnings,
    };

    if !clean {
        return Err(CliError::UserError {
            command: COMMAND_PR.into(),
            message: format!(
                "{} blocking finding(s) ({} warning(s))",
                outcome.report.summary.errors, outcome.report.summary.warnings
            ),
            details: serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null),
        });
    }

    Ok(CommandOutcome::CheckPr(outcome))
}

/// `firetrail check paths` — per-commit validator over an explicit path list.
pub fn paths(args: &CheckPathsArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND_PATHS, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let report = validate_pre_commit(&ctx.storage, &args.paths);
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

    let outcome = CheckPathsOutcome {
        clean,
        memory_only,
        rows,
        warnings,
    };

    if !clean {
        return Err(CliError::UserError {
            command: COMMAND_PATHS.into(),
            message: format!(
                "{} record file(s) failed pre-commit validation",
                outcome.rows.iter().filter(|r| !r.ok).count()
            ),
            details: serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null),
        });
    }

    Ok(CommandOutcome::CheckPaths(outcome))
}

fn class_label(c: &ChangeClass) -> &'static str {
    match c {
        ChangeClass::Memory(_) => "memory",
        ChangeClass::Structural(_) => "structural",
        ChangeClass::Config => "config",
        ChangeClass::Other => "other",
    }
}

/// Outcome of `firetrail check pr`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckPrOutcome {
    /// Base git ref.
    pub base: String,
    /// Head git ref.
    pub head: String,
    /// Whether `--strict` was supplied.
    pub strict: bool,
    /// Whether the secret-scan rule ran.
    pub secret_scan_enabled: bool,
    /// Full ft-pr report payload.
    pub report: PrReport,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl CheckPrOutcome {
    /// Markdown rendering for human consumption.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let summary = &self.report.summary;
        let mut s = format!(
            "# check pr `{}..{}`\n\n\
             - changed records: {}\n\
             - errors: {}\n\
             - warnings: {}\n\
             - strict: {}\n\
             - secret_scan: {}\n",
            self.base,
            self.head,
            summary.changed_records,
            summary.errors,
            summary.warnings,
            self.strict,
            self.secret_scan_enabled,
        );
        if !self.report.findings.is_empty() {
            s.push_str(
                "\n## Findings\n\n| Severity | Rule | Record | Message |\n|---|---|---|---|\n",
            );
            for f in &self.report.findings {
                let id = f
                    .record_id
                    .as_ref()
                    .map_or_else(|| "—".to_string(), |i| i.as_str().to_string());
                let sev = match f.severity {
                    ft_pr::Severity::Error => "error",
                    ft_pr::Severity::Warning => "warning",
                    ft_pr::Severity::Info => "info",
                };
                let rule = serde_json::to_value(f.rule)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{:?}", f.rule));
                let _ = writeln!(s, "| {} | `{}` | `{}` | {} |", sev, rule, id, f.message);
            }
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "check pr: errors={} warnings={} changed={}",
            self.report.summary.errors,
            self.report.summary.warnings,
            self.report.summary.changed_records,
        )
    }
}

/// Per-path row in the `check paths` report.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRow {
    /// Path under the repo root.
    pub path: String,
    /// Coarse classification (`memory`, `structural`, `config`, `other`).
    pub class: String,
    /// `true` when validation succeeded (or the path was a no-op).
    pub ok: bool,
    /// First failure reason, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Outcome of `firetrail check paths`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckPathsOutcome {
    /// `true` iff every input record file validated.
    pub clean: bool,
    /// `true` iff every input is a memory-kind record file (ADR-0009).
    pub memory_only: bool,
    /// Per-path verdicts.
    pub rows: Vec<CheckRow>,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl CheckPathsOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# check paths\n\nClean: {} · Memory-only: {} · Paths: {}\n",
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
                    if r.ok { "ok" } else { "FAIL" },
                    r.reason.as_deref().unwrap_or("—"),
                );
            }
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "check paths: clean={} memory_only={} ({} paths)",
            self.clean,
            self.memory_only,
            self.rows.len()
        )
    }
}
