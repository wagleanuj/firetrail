//! `firetrail lint memory` — static lint over the current workspace state.
//!
//! Runs the subset of ft-pr rules that apply without a base/head pair:
//!
//! - [`ft_pr::RuleId::AcCapExceeded`]
//! - [`ft_pr::RuleId::DraftExpired`]
//! - [`ft_pr::RuleId::DeprecatedReference`]
//! - [`ft_pr::RuleId::ChainBroken`] (via [`ft_history::verify_chain`])
//! - [`ft_pr::RuleId::SecretLeak`]
//!
//! Diff-dependent rules (`MixedCommit`, `PrLinkMissing`,
//! `IncompleteAcceptance`, `EvidenceRequired`) require a base/head — they
//! are skipped here. Run `firetrail check pr` for those.

use std::path::Path;

use chrono::Utc;
use ft_core::{Record, RecordBody, Status, TrustState};
use ft_history::verify_chain;
use ft_pr::{PrValidatorOptions, default_secret_patterns};
use ft_storage::Storage as _;
use regex::Regex;
use serde::Serialize;

use crate::cli::{GlobalOpts, LintMemoryArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "lint memory";

/// One lint finding.
#[derive(Debug, Clone, Serialize)]
pub struct LintFinding {
    /// Severity (`error` / `warning`).
    pub severity: &'static str,
    /// Rule id (matches [`ft_pr::RuleId`] discriminant in `snake_case`).
    pub rule: &'static str,
    /// Record id the finding pertains to.
    pub record_id: String,
    /// Human-readable message.
    pub message: String,
    /// Suggested remediation. Populated when `--fix` is passed.
    /// No auto-fix is applied automatically — every current lint rule either
    /// touches integrity-critical fields (`state_hash`, trust transitions) or
    /// requires human judgment.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub suggested_fix: Option<String>,
}

/// `firetrail lint memory` — workspace-state lint without a base/head diff.
#[allow(clippy::too_many_lines)]
pub fn memory(args: &LintMemoryArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let emit_fix_hints = args.fix;

    let opts = PrValidatorOptions::default();
    let patterns = if opts.enable_secret_scan {
        default_secret_patterns()
    } else {
        Vec::new()
    };

    let records_root = ctx.storage.records_root();
    let raw_records = walk_records(&records_root);

    let mut findings = Vec::<LintFinding>::new();
    let mut errors = 0usize;

    // Pre-cache records for cross-reference checks (only the parseable ones).
    let mut by_id: std::collections::HashMap<ft_core::RecordId, Record> =
        std::collections::HashMap::with_capacity(raw_records.len());

    // First pass: walk raw bytes. A record that fails to parse, or whose
    // recomputed state_hash disagrees with the on-disk value, surfaces as a
    // ChainBroken finding (per ADR-0017's tamper-detection semantics).
    let mut scanned = 0usize;
    for entry in &raw_records {
        scanned += 1;
        match parse_and_verify(entry) {
            Ok(record) => {
                by_id.insert(record.envelope.id.clone(), record);
            }
            Err((id, reason)) => {
                errors += 1;
                findings.push(LintFinding {
                    severity: "error",
                    rule: "chain_broken",
                    record_id: id,
                    message: format!("chain integrity failure: {reason}"),
                    suggested_fix: emit_fix_hints.then(|| {
                        "tamper detected — restore record from git history or rebuild via `firetrail index rebuild`".to_string()
                    }),
                });
            }
        }
    }

    for record in by_id.values() {
        let record_id = record.envelope.id.as_str().to_string();

        // AcCapExceeded
        let acs = ac_count(record);
        if acs > opts.max_ac_per_record {
            findings.push(LintFinding {
                severity: "warning",
                rule: "ac_cap_exceeded",
                record_id: record_id.clone(),
                message: format!(
                    "record has {} acceptance criteria (cap: {})",
                    acs, opts.max_ac_per_record
                ),
                suggested_fix: emit_fix_hints.then(|| {
                    format!(
                        "split into subtasks via `firetrail subtask create --parent {record_id}` or remove low-value criteria"
                    )
                }),
            });
        }

        // DraftExpired
        if let Some(TrustState::Draft) = memory_trust(record) {
            let age = Utc::now().signed_duration_since(record.envelope.created_at);
            let days = age.num_days();
            if days > opts.draft_max_age_days {
                findings.push(LintFinding {
                    severity: "warning",
                    rule: "draft_expired",
                    record_id: record_id.clone(),
                    message: format!(
                        "draft record is {} days old (expiry: {} days)",
                        days, opts.draft_max_age_days
                    ),
                    suggested_fix: emit_fix_hints.then(|| {
                        format!(
                            "promote via `firetrail memory review {record_id}` or archive via `firetrail memory archive {record_id}`"
                        )
                    }),
                });
            }
        }

        // DeprecatedReference
        for r in collect_references(record) {
            if let Some(target) = by_id.get(&r) {
                if let Some(state) = deprecated_state(target) {
                    let target_id = target.envelope.id.as_str().to_string();
                    findings.push(LintFinding {
                        severity: "warning",
                        rule: "deprecated_reference",
                        record_id: record_id.clone(),
                        message: format!(
                            "references {target_id} which is in state `{state}`"
                        ),
                        suggested_fix: emit_fix_hints.then(|| {
                            format!(
                                "update {record_id} to point at the successor of {target_id}, or remove the reference"
                            )
                        }),
                    });
                }
            }
        }

        // SecretLeak
        if !patterns.is_empty() {
            if let Some((matched, pat)) = scan_secrets(record, &patterns) {
                errors += 1;
                findings.push(LintFinding {
                    severity: "error",
                    rule: "secret_leak",
                    record_id: record_id.clone(),
                    message: format!(
                        "possible secret detected (pattern: `{}`, preview: `{}`)",
                        pat.as_str(),
                        redact(&matched)
                    ),
                    suggested_fix: emit_fix_hints.then(|| {
                        format!(
                            "redact via `firetrail memory redact {record_id} --reason \"contained secret\"` and rotate the credential"
                        )
                    }),
                });
            }
        }
    }

    let outcome = LintMemoryOutcome {
        scanned,
        errors,
        warnings_count: findings.iter().filter(|f| f.severity == "warning").count(),
        findings,
        warnings,
    };

    if outcome.errors > 0 {
        return Err(CliError::UserError {
            command: COMMAND.into(),
            message: format!("{} error-severity lint finding(s)", outcome.errors),
            details: serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null),
        });
    }
    Ok(CommandOutcome::LintMemory(outcome))
}

/// Outcome of `firetrail lint memory`.
#[derive(Debug, Clone, Serialize)]
pub struct LintMemoryOutcome {
    /// Number of records scanned.
    pub scanned: usize,
    /// Total error-severity findings.
    pub errors: usize,
    /// Total warning-severity findings.
    pub warnings_count: usize,
    /// Per-finding rows.
    pub findings: Vec<LintFinding>,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl LintMemoryOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# lint memory\n\nScanned {} record(s); {} error(s), {} warning(s).\n",
            self.scanned, self.errors, self.warnings_count
        );
        if !self.findings.is_empty() {
            s.push_str("\n| Severity | Rule | Record | Message |\n|---|---|---|---|\n");
            for f in &self.findings {
                let _ = writeln!(
                    s,
                    "| {} | `{}` | `{}` | {} |",
                    f.severity, f.rule, f.record_id, f.message
                );
            }
            let with_hints: Vec<&LintFinding> =
                self.findings.iter().filter(|f| f.suggested_fix.is_some()).collect();
            if !with_hints.is_empty() {
                s.push_str("\n## Suggested fixes\n\n");
                for f in with_hints {
                    if let Some(hint) = &f.suggested_fix {
                        let _ = writeln!(s, "- `{}` ({}): {}", f.record_id, f.rule, hint);
                    }
                }
            }
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "lint memory: scanned={} errors={} warnings={}",
            self.scanned, self.errors, self.warnings_count
        )
    }
}

// ---------------------------------------------------------------------------
// rule-extracted helpers (mirroring ft-pr's pub(crate) helpers)
// ---------------------------------------------------------------------------

fn ac_count(record: &Record) -> usize {
    match &record.body {
        RecordBody::Task(t) => t.acceptance_criteria.len(),
        RecordBody::Subtask(s) => s.acceptance_criteria.len(),
        RecordBody::Bug(b) => b.acceptance_criteria.len(),
        _ => 0,
    }
}

fn memory_trust(record: &Record) -> Option<TrustState> {
    Some(match &record.body {
        RecordBody::Incident(b) => b.trust,
        RecordBody::Finding(b) => b.trust,
        RecordBody::Runbook(b) => b.trust,
        RecordBody::Decision(b) => b.trust,
        RecordBody::Gotcha(b) => b.trust,
        RecordBody::Memory(b) => b.trust,
        _ => return None,
    })
}

fn collect_references(record: &Record) -> Vec<ft_core::RecordId> {
    let mut out = Vec::new();
    match &record.body {
        RecordBody::Task(t) => {
            if let Some(p) = &t.parent_epic {
                out.push(p.clone());
            }
        }
        RecordBody::Subtask(s) => out.push(s.parent_task.clone()),
        RecordBody::Incident(i) => {
            out.extend(i.findings.iter().cloned());
            out.extend(i.runbooks_invoked.iter().cloned());
        }
        RecordBody::Finding(f) => {
            if let Some(i) = &f.incident {
                out.push(i.clone());
            }
            if let Some(s) = &f.superseded_by {
                out.push(s.clone());
            }
        }
        RecordBody::Decision(d) => {
            if let Some(s) = &d.superseded_by {
                out.push(s.clone());
            }
        }
        RecordBody::Memory(m) => out.extend(m.related.iter().cloned()),
        _ => {}
    }
    out
}

fn deprecated_state(record: &Record) -> Option<&'static str> {
    if record.envelope.status == Status::Archived {
        return Some("archived");
    }
    let trust = match &record.body {
        RecordBody::Incident(b) => b.trust,
        RecordBody::Finding(b) => b.trust,
        RecordBody::Runbook(b) => b.trust,
        RecordBody::Decision(b) => b.trust,
        RecordBody::Gotcha(b) => b.trust,
        RecordBody::Memory(b) => b.trust,
        _ => return None,
    };
    match trust {
        TrustState::Deprecated => Some("deprecated"),
        TrustState::Archived => Some("archived"),
        TrustState::Rejected => Some("rejected"),
        TrustState::Redacted => Some("redacted"),
        TrustState::Superseded => Some("superseded"),
        _ => None,
    }
}

const SKIP_FIELDS: &[&str] = &[
    "state_hash",
    "prev_state_hash",
    "from_hash",
    "to_hash",
    "id",
    "content_hash",
    "commit_sha",
];

fn scan_secrets<'p>(record: &Record, patterns: &'p [Regex]) -> Option<(String, &'p Regex)> {
    let value = serde_json::to_value(record).ok()?;
    let mut hit: Option<(String, &Regex)> = None;
    scan(&value, patterns, &mut hit);
    hit
}

fn scan<'p>(
    value: &serde_json::Value,
    patterns: &'p [Regex],
    hit: &mut Option<(String, &'p Regex)>,
) {
    if hit.is_some() {
        return;
    }
    match value {
        serde_json::Value::String(s) => {
            for p in patterns {
                if let Some(m) = p.find(s) {
                    *hit = Some((m.as_str().to_string(), p));
                    return;
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                scan(v, patterns, hit);
                if hit.is_some() {
                    return;
                }
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if SKIP_FIELDS.contains(&k.as_str()) {
                    continue;
                }
                scan(v, patterns, hit);
                if hit.is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

/// Walk every `<records_root>/<kind>/*.json` file. Mirrors
/// `commands::verify::walk_record_files` so a tampered record file does not
/// short-circuit the scan.
fn walk_records(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(top) = std::fs::read_dir(root) else {
        return out;
    };
    for kind_entry in top.flatten() {
        if !kind_entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(kind_entry.path()) else {
            continue;
        };
        for file in files.flatten() {
            let p = file.path();
            if p.extension().is_some_and(|e| e == "json") {
                out.push(p);
            }
        }
    }
    out
}

/// Read the record file at `path`, validate `state_hash`, and run
/// `verify_chain`. Returns the record on success, or `(id, reason)` on the
/// first failure encountered.
fn parse_and_verify(path: &Path) -> Result<Record, (String, String)> {
    let bytes =
        std::fs::read(path).map_err(|e| (path.display().to_string(), format!("read: {e}")))?;
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|e| (path.display().to_string(), format!("parse: {e}")))?;
    let id = record.envelope.id.as_str().to_string();

    // Tamper detection mirrors `commands::verify`: recompute the closed-form
    // hash and compare to the on-disk value before deferring to the full
    // chain verifier.
    let recomputed =
        ft_core::state_hash(&record).map_err(|e| (id.clone(), format!("hash recompute: {e}")))?;
    if recomputed != record.envelope.state_hash {
        return Err((
            id,
            format!(
                "state_hash mismatch: stored={} recomputed={}",
                record.envelope.state_hash, recomputed
            ),
        ));
    }
    verify_chain(&record).map_err(|e| (id.clone(), e.to_string()))?;
    Ok(record)
}

fn redact(s: &str) -> String {
    let len = s.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    let prefix = &s[..4];
    let suffix = &s[len.saturating_sub(2)..];
    format!("{prefix}…{suffix}")
}
