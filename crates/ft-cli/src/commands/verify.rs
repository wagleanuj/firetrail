//! `firetrail verify [<id>] [--all]` — walk per-record history chains and
//! report tampering (force-pushes, hash mismatches, broken links).
//!
//! Wraps [`ft_history::verify_chain`] and routes its precise errors into
//! the JSON envelope. A single-id call exits non-zero (`UserError`) on the
//! first failure; the all-records pass aggregates and surfaces every
//! offender at once so operators can triage in one round-trip.

use ft_core::Record;
use ft_history::verify_chain;
use ft_storage::Storage as _;
use serde::Serialize;

use crate::cli::{GlobalOpts, VerifyArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "verify";

/// `firetrail verify`
pub fn run(args: &VerifyArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let mut results: Vec<VerifyResult> = Vec::new();
    let mut failures = 0usize;

    if let Some(raw) = &args.id {
        let id = ctx.resolve_id(raw)?;
        let record = ctx.read_record(&id)?;
        let (ok, reason) = match verify_chain(&record) {
            Ok(()) => (true, None),
            Err(e) => {
                failures += 1;
                (false, Some(e.to_string()))
            }
        };
        results.push(VerifyResult {
            id: id.as_str().to_string(),
            ok,
            reason,
        });
    } else {
        // Walk every record file directly on disk so a hash-mismatched
        // record surfaces as a per-record failure rather than aborting
        // the whole pass. `Storage::list` validates every record on read,
        // which is the wrong semantics for the verify command — verify
        // *expects* corruption and wants to report it precisely.
        let root = ctx.storage.records_root();
        for entry in walk_record_files(&root) {
            let path = match entry {
                Ok(p) => p,
                Err(e) => {
                    failures += 1;
                    results.push(VerifyResult {
                        id: String::new(),
                        ok: false,
                        reason: Some(format!("walk: {e}")),
                    });
                    continue;
                }
            };
            let (id_str, ok, reason) = inspect_path(&path);
            if !ok {
                failures += 1;
            }
            results.push(VerifyResult {
                id: id_str,
                ok,
                reason,
            });
        }
    }

    let report = VerifyOutcome {
        total: results.len(),
        failures,
        results,
        warnings,
    };

    if failures > 0 {
        // Surface as a user error so the exit code reflects the failure,
        // but keep the structured report on the `details` payload so JSON
        // consumers still get the full picture.
        return Err(CliError::UserError {
            command: COMMAND.into(),
            message: format!(
                "{} record(s) failed chain verification (of {} checked)",
                report.failures, report.total
            ),
            details: serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        });
    }

    Ok(CommandOutcome::Verify(report))
}

/// Per-record verdict.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    /// Canonical record id.
    pub id: String,
    /// `true` if the chain verified.
    pub ok: bool,
    /// First failure reason (`None` when `ok`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Aggregate report rendered by `firetrail verify`.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyOutcome {
    /// Total records inspected.
    pub total: usize,
    /// Records that failed verification.
    pub failures: usize,
    /// Per-record verdicts.
    pub results: Vec<VerifyResult>,
    /// Non-fatal CLI warnings (e.g. index auto-rebuild).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl VerifyOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# verify\n\nChecked {} record(s); {} failure(s).\n",
            self.total, self.failures
        );
        if !self.results.is_empty() {
            s.push_str("\n| ID | OK | Reason |\n|----|----|--------|\n");
            for r in &self.results {
                let _ = writeln!(
                    s,
                    "| `{}` | {} | {} |",
                    r.id,
                    if r.ok { "✓" } else { "✗" },
                    r.reason.as_deref().unwrap_or("—")
                );
            }
        }
        s
    }

    /// One-line summary for `--quiet`.
    pub fn quiet_line(&self) -> String {
        format!(
            "verify: {}/{} clean",
            self.total - self.failures,
            self.total
        )
    }
}

/// Walk every `<root>/<kind>/*.json` record file, returning paths only.
fn walk_record_files(root: &std::path::Path) -> Vec<std::io::Result<std::path::PathBuf>> {
    let mut out = Vec::new();
    let Ok(top) = std::fs::read_dir(root) else {
        return out;
    };
    for kind_entry in top {
        let kind_entry = match kind_entry {
            Ok(e) => e,
            Err(e) => {
                out.push(Err(e));
                continue;
            }
        };
        if !kind_entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(kind_entry.path()) else {
            continue;
        };
        for file in files {
            let file = match file {
                Ok(f) => f,
                Err(e) => {
                    out.push(Err(e));
                    continue;
                }
            };
            let p = file.path();
            if p.extension().is_some_and(|e| e == "json") {
                out.push(Ok(p));
            }
        }
    }
    out
}

/// Read a record file directly and verify it without going through
/// `Storage::read` (which would refuse hash-mismatched records).
fn inspect_path(path: &std::path::Path) -> (String, bool, Option<String>) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return (
                path.display().to_string(),
                false,
                Some(format!("read: {e}")),
            );
        }
    };
    let record: Record = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => {
            return (
                path.display().to_string(),
                false,
                Some(format!("parse: {e}")),
            );
        }
    };
    let id = record.envelope.id.as_str().to_string();

    // Recompute the closed-form state_hash; mismatch is the on-disk
    // tamper signal (force-push detection per ADR-0017).
    let recomputed = match ft_core::state_hash(&record) {
        Ok(h) => h,
        Err(e) => return (id, false, Some(format!("hash recompute: {e}"))),
    };
    if recomputed != record.envelope.state_hash {
        return (
            id,
            false,
            Some(format!(
                "state_hash mismatch: stored={} recomputed={}",
                record.envelope.state_hash, recomputed
            )),
        );
    }

    match verify_chain(&record) {
        Ok(()) => (id, true, None),
        Err(e) => (id, false, Some(e.to_string())),
    }
}
