//! ADR-0013: high-stakes memory records promoted to [`TrustState::Verified`]
//! in this PR must carry at least one piece of evidence.
//!
//! The audit-trail unit for trust transitions is `ft_trust::TrustTransition`,
//! but `ft-core::HistoryEntry` does not yet carry a structured transition
//! payload. We detect promotion by comparing base-vs-head `trust` on the
//! memory body and require an `evidence:` marker in the head's last
//! [`HistoryEntry::ops_summary`].

use ft_core::{Record, RecordBody, RiskClass, TrustState};

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    for c in cx.changed {
        let Some(head) = c.at_head.as_ref() else {
            continue;
        };
        let Some((trust_head, risk_head)) = memory_trust_and_risk(head) else {
            continue;
        };
        if trust_head != TrustState::Verified {
            continue;
        }
        if !risk_head.is_some_and(RiskClass::is_high_stakes) {
            continue;
        }
        let transitioned = c
            .at_base
            .as_ref()
            .and_then(memory_trust_and_risk)
            .is_none_or(|(t, _)| t != TrustState::Verified);
        if !transitioned {
            continue;
        }

        if !has_evidence_marker(head) {
            report.push(PrFinding {
                severity: Severity::Error,
                rule: RuleId::EvidenceRequired,
                record_id: Some(head.envelope.id.clone()),
                path: Some(c.path.clone()),
                message:
                    "high-stakes record promoted to Verified without recorded evidence (ADR-0013)"
                        .to_string(),
                details: serde_json::json!({
                    "risk_class": risk_head,
                }),
            });
        }
    }
}

fn memory_trust_and_risk(record: &Record) -> Option<(TrustState, Option<RiskClass>)> {
    Some(match &record.body {
        RecordBody::Incident(b) => (b.trust, b.risk_class),
        RecordBody::Finding(b) => (b.trust, b.risk_class),
        RecordBody::Runbook(b) => (b.trust, b.risk_class),
        RecordBody::Decision(b) => (b.trust, b.risk_class),
        RecordBody::Gotcha(b) => (b.trust, b.risk_class),
        RecordBody::Memory(b) => (b.trust, b.risk_class),
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            return None;
        }
    })
}

/// `ft-cli` writes a history-entry `ops_summary` line beginning with the
/// case-insensitive prefix `evidence:` when a reviewer attaches evidence.
/// Until `HistoryEntry` carries a structured transition payload (follow-up),
/// this marker is the contract `ft-pr` checks for.
fn has_evidence_marker(record: &Record) -> bool {
    record
        .envelope
        .history
        .last()
        .is_some_and(|h| h.ops_summary.iter().any(|s| has_evidence_prefix(s)))
}

fn has_evidence_prefix(s: &str) -> bool {
    // History entries are prefixed with the kind tag (e.g. `update: `) by
    // `ft-history`. The evidence marker may appear at the start of the line
    // *or* after the kind tag. Match case-insensitive on the substring
    // `evidence:` to keep the convention permissive.
    let lower = s.to_ascii_lowercase();
    lower.contains("evidence:")
}
