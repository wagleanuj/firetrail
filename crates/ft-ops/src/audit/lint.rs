//! `lint` op — static lint over the current workspace state.
//!
//! Mirrors `ft_cli::commands::lint::memory` but emits a structured
//! [`LintOutput`] designed for the GUI. Findings carry suggested-fix hints
//! (per firetrail-clv) whenever the input opts in via `fix_hints`.

use std::path::Path;

use chrono::Utc;
use ft_core::{Record, RecordBody, Status, TrustState};
use ft_history::verify_chain;
use ft_pr::{PrValidatorOptions, default_secret_patterns};
use ft_storage::{EmbeddedStorage, Storage as _};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Severity of a lint finding.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    /// Error — surfaces as a blocking issue.
    Error,
    /// Warning — informational only.
    Warning,
}

/// One lint finding.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintFinding {
    /// Severity bucket.
    pub severity: LintSeverity,
    /// Rule id (e.g. `"ac_cap_exceeded"`).
    pub rule: String,
    /// Record id (or file path when the record failed to parse).
    pub record_id: String,
    /// Human-readable message.
    pub message: String,
    /// Suggested remediation. Populated when `fix_hints` was on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<String>,
}

/// Input for [`lint`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "LintInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintInput {
    /// Emit suggested-fix hints on every finding.
    #[serde(default)]
    pub fix_hints: bool,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`lint`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "LintOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintOutput {
    /// Records scanned.
    pub scanned: usize,
    /// Number of error-severity findings.
    pub errors: usize,
    /// Number of warning-severity findings.
    pub warnings: usize,
    /// All findings.
    pub findings: Vec<LintFinding>,
}

/// `lint` op.
#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
pub fn lint(
    ws: &Workspace,
    _identity: &Identity,
    input: LintInput,
    events: &EventBus,
) -> Result<LintOutput, OpsError> {
    let storage = EmbeddedStorage::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open storage: {e}")))?;

    let emit_fix_hints = input.fix_hints;
    let opts = PrValidatorOptions::default();
    let patterns = if opts.enable_secret_scan {
        default_secret_patterns()
    } else {
        Vec::new()
    };

    let records_root = storage.records_root();
    let raw_records = walk_records(&records_root);

    let mut findings = Vec::<LintFinding>::new();
    let mut errors = 0usize;

    let mut by_id: std::collections::HashMap<ft_core::RecordId, Record> =
        std::collections::HashMap::with_capacity(raw_records.len());

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
                    severity: LintSeverity::Error,
                    rule: "chain_broken".into(),
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

        let acs = ac_count(record);
        if acs > opts.max_ac_per_record {
            findings.push(LintFinding {
                severity: LintSeverity::Warning,
                rule: "ac_cap_exceeded".into(),
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

        if let Some(TrustState::Draft) = memory_trust(record) {
            let age = Utc::now().signed_duration_since(record.envelope.created_at);
            let days = age.num_days();
            if days > opts.draft_max_age_days {
                findings.push(LintFinding {
                    severity: LintSeverity::Warning,
                    rule: "draft_expired".into(),
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

        for r in collect_references(record) {
            if let Some(target) = by_id.get(&r) {
                if let Some(state) = deprecated_state(target) {
                    let target_id = target.envelope.id.as_str().to_string();
                    findings.push(LintFinding {
                        severity: LintSeverity::Warning,
                        rule: "deprecated_reference".into(),
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

        if !patterns.is_empty() {
            if let Some((matched, pat)) = scan_secrets(record, &patterns) {
                errors += 1;
                findings.push(LintFinding {
                    severity: LintSeverity::Error,
                    rule: "secret_leak".into(),
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

    let warnings = findings
        .iter()
        .filter(|f| matches!(f.severity, LintSeverity::Warning))
        .count();
    let total = findings.len();

    let event = Event::LintRun { findings: total };
    if let Some(rid) = input.request_id.as_deref() {
        events.emit_with_request(rid.to_string(), event);
    } else {
        events.emit(event);
    }

    Ok(LintOutput {
        scanned,
        errors,
        warnings,
        findings,
    })
}

// ---------------------------------------------------------------------------
// helpers (mirroring ft_cli::commands::lint)
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

pub(crate) fn walk_records(root: &Path) -> Vec<std::path::PathBuf> {
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

fn parse_and_verify(path: &Path) -> Result<Record, (String, String)> {
    let bytes =
        std::fs::read(path).map_err(|e| (path.display().to_string(), format!("read: {e}")))?;
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|e| (path.display().to_string(), format!("parse: {e}")))?;
    let id = record.envelope.id.as_str().to_string();

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
