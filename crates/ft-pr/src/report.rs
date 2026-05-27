//! Structured output of [`crate::validate_pr`].
//!
//! The CLI renders a [`PrReport`] to JSON or to a human-readable summary. The
//! shape of the report is part of the public API; CI scripts depend on
//! [`PrReport::summary`] counts and on [`PrFinding::rule`] discriminants.

use std::path::PathBuf;

use ft_core::RecordId;
use serde::{Deserialize, Serialize};

/// Aggregate counts produced by a single validation pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrSummary {
    /// Number of distinct record files in the diff (Added / Modified /
    /// Renamed; deletes are counted too because they can violate referential
    /// integrity).
    pub changed_records: usize,
    /// Total Error-severity findings.
    pub errors: usize,
    /// Total Warning-severity findings.
    pub warnings: usize,
}

/// Full validation output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrReport {
    /// High-level counts.
    pub summary: PrSummary,
    /// All findings, ordered by rule then path.
    pub findings: Vec<PrFinding>,
}

impl PrReport {
    /// `true` iff there are no Error-severity findings (and, if `strict`, no
    /// Warning-severity findings either).
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.summary.errors == 0
    }

    /// Add a finding and update counters.
    pub(crate) fn push(&mut self, finding: PrFinding) {
        match finding.severity {
            Severity::Error => self.summary.errors += 1,
            Severity::Warning => self.summary.warnings += 1,
            Severity::Info => {}
        }
        self.findings.push(finding);
    }
}

/// Severity of a single [`PrFinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Blocking violation; the PR is not clean.
    Error,
    /// Non-blocking violation; visible in the report but does not fail by
    /// default. Promoted to blocking under [`crate::PrValidatorOptions::strict`].
    Warning,
    /// Informational only.
    Info,
}

/// Rule identifier surfaced on [`PrFinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleId {
    /// ADR-0009: code and memory records in the same commit.
    MixedCommit,
    /// ADR-0010: task/subtask/bug closed with unchecked acceptance criteria.
    IncompleteAcceptance,
    /// ADR-0013: high-stakes record promoted to Verified without evidence.
    EvidenceRequired,
    /// ADR-0017: `state_hash` chain integrity broken.
    ChainBroken,
    /// Secret-scan pattern hit in record body.
    SecretLeak,
    /// AC count exceeds the configured cap.
    AcCapExceeded,
    /// Draft record older than the configured expiry.
    DraftExpired,
    /// Reference points at a Deprecated / Archived / Rejected / Redacted record.
    DeprecatedReference,
    /// ADR-0010: PR description references a record but its closure cannot be
    /// resolved in the diff.
    PrLinkMissing,
    /// ADR-0021 pilot rollout: a record was skipped because its
    /// `owning_scope` is not in `scopes.yaml::enabled_scopes`. Always
    /// `Info`-severity — surfaces the skip so CI logs explain why a
    /// would-be finding did not fire.
    ScopeSkipped,
}

/// A single rule violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFinding {
    /// Severity of this violation.
    pub severity: Severity,
    /// Which rule produced it.
    pub rule: RuleId,
    /// Record the finding pertains to (None for repo-level findings, e.g.
    /// mixed-commit).
    pub record_id: Option<RecordId>,
    /// Repo-relative path the finding pertains to, when applicable.
    pub path: Option<PathBuf>,
    /// Human-readable, one-line description suitable for CI output.
    pub message: String,
    /// Rule-specific structured details, for machine consumers (`firetrail
    /// check pr --json`).
    pub details: serde_json::Value,
}
