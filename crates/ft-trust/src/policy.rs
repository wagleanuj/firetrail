//! Stale-record policy and computation.
//!
//! Records age out per ADR-0013. The thresholds are per-kind because the
//! cadence at which a Finding becomes stale ("90 days unverified") differs
//! from a Runbook ("180 days unrun") or a Decision (decisions don't go stale
//! the same way — they get superseded).
//!
//! The defaults below are starting points; callers may override via
//! [`StalePolicy::with_threshold`].

use chrono::{DateTime, Duration, Utc};
use ft_core::{Record, RecordBody, RecordKind, RiskClass, TrustState};

/// Threshold for high-stakes records that haven't been re-verified, per
/// ADR-0013 ("Re-validation every 180 days").
pub const HIGH_STAKES_REVALIDATION_DAYS: i64 = 180;

/// Per-kind staleness thresholds in days.
///
/// `None` means "never stale through this rule alone" — used for kinds whose
/// content posture is governed by `DecisionStatus::Superseded` or similar
/// content-level lifecycle, not by raw age.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePolicy {
    /// Threshold for [`RecordKind::Finding`] in days.
    pub finding_days: Option<i64>,
    /// Threshold for [`RecordKind::Runbook`] in days.
    pub runbook_days: Option<i64>,
    /// Threshold for [`RecordKind::Decision`] in days.
    pub decision_days: Option<i64>,
    /// Threshold for [`RecordKind::Gotcha`] in days.
    pub gotcha_days: Option<i64>,
    /// Threshold for [`RecordKind::Memory`] in days.
    pub memory_days: Option<i64>,
    /// Threshold for [`RecordKind::Incident`] in days.
    pub incident_days: Option<i64>,
    /// Threshold for high-stakes records (overrides the per-kind threshold
    /// when the record's `risk_class` is high-stakes).
    pub high_stakes_days: i64,
}

impl Default for StalePolicy {
    /// Sensible defaults per ADR-0013:
    ///
    /// - Finding: 90 days
    /// - Runbook: 180 days
    /// - Gotcha: 365 days
    /// - Memory: 365 days
    /// - Incident: never (incidents are point-in-time historical records)
    /// - Decision: never (lifecycle governed by `DecisionStatus`)
    /// - High-stakes override: 180 days
    fn default() -> Self {
        Self {
            finding_days: Some(90),
            runbook_days: Some(180),
            decision_days: None,
            gotcha_days: Some(365),
            memory_days: Some(365),
            incident_days: None,
            high_stakes_days: HIGH_STAKES_REVALIDATION_DAYS,
        }
    }
}

impl StalePolicy {
    /// Override the threshold for a single [`RecordKind`].
    #[must_use]
    pub fn with_threshold(mut self, kind: RecordKind, days: Option<i64>) -> Self {
        match kind {
            RecordKind::Finding => self.finding_days = days,
            RecordKind::Runbook => self.runbook_days = days,
            RecordKind::Decision => self.decision_days = days,
            RecordKind::Gotcha => self.gotcha_days = days,
            RecordKind::Memory => self.memory_days = days,
            RecordKind::Incident => self.incident_days = days,
            // Non-memory kinds have no trust-age rule.
            RecordKind::Epic | RecordKind::Task | RecordKind::Subtask | RecordKind::Bug => {}
        }
        self
    }

    fn threshold_for(&self, kind: RecordKind) -> Option<i64> {
        match kind {
            RecordKind::Finding => self.finding_days,
            RecordKind::Runbook => self.runbook_days,
            RecordKind::Decision => self.decision_days,
            RecordKind::Gotcha => self.gotcha_days,
            RecordKind::Memory => self.memory_days,
            RecordKind::Incident => self.incident_days,
            RecordKind::Epic | RecordKind::Task | RecordKind::Subtask | RecordKind::Bug => None,
        }
    }
}

/// Returns `true` if `record` should be considered stale at `now` under
/// `policy`.
///
/// Rules:
///
/// 1. Already-stale records stay stale.
/// 2. Terminal states (Archived, Superseded, Rejected, Redacted) are *not*
///    stale — they have their own end-of-life semantics.
/// 3. For memory-kind records, the age (`now - updated_at`) is compared
///    against the per-kind threshold. High-stakes records (per
///    [`RiskClass::is_high_stakes`]) use the shorter of (per-kind,
///    `high_stakes_days`).
/// 4. Records whose threshold is `None` and which aren't high-stakes never go
///    stale through this rule.
#[must_use]
pub fn is_stale(record: &Record, now: DateTime<Utc>, policy: &StalePolicy) -> bool {
    let trust = body_trust(&record.body);

    if matches!(trust, Some(TrustState::Stale)) {
        return true;
    }
    if trust.is_some_and(is_terminal_trust) {
        return false;
    }

    let kind = record.envelope.kind;
    let per_kind = policy.threshold_for(kind);
    let risk = body_risk(&record.body);
    let high = risk.is_some_and(RiskClass::is_high_stakes);

    let threshold_days = match (per_kind, high) {
        (Some(days), true) => Some(days.min(policy.high_stakes_days)),
        (Some(days), false) => Some(days),
        (None, true) => Some(policy.high_stakes_days),
        (None, false) => None,
    };

    let Some(days) = threshold_days else {
        return false;
    };

    let age = now.signed_duration_since(record.envelope.updated_at);
    age >= Duration::days(days)
}

fn is_terminal_trust(t: TrustState) -> bool {
    matches!(
        t,
        TrustState::Archived | TrustState::Superseded | TrustState::Rejected | TrustState::Redacted
    )
}

fn body_trust(body: &RecordBody) -> Option<TrustState> {
    match body {
        RecordBody::Incident(b) => Some(b.trust),
        RecordBody::Finding(b) => Some(b.trust),
        RecordBody::Runbook(b) => Some(b.trust),
        RecordBody::Decision(b) => Some(b.trust),
        RecordBody::Gotcha(b) => Some(b.trust),
        RecordBody::Memory(b) => Some(b.trust),
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            None
        }
    }
}

fn body_risk(body: &RecordBody) -> Option<RiskClass> {
    match body {
        RecordBody::Incident(b) => b.risk_class,
        RecordBody::Finding(b) => b.risk_class,
        RecordBody::Runbook(b) => b.risk_class,
        RecordBody::Decision(b) => b.risk_class,
        RecordBody::Gotcha(b) => b.risk_class,
        RecordBody::Memory(b) => b.risk_class,
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            None
        }
    }
}
