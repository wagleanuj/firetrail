//! ADR-0013: high-stakes memory records promoted to [`TrustState::Verified`]
//! in this PR must carry at least one piece of evidence.
//!
//! We detect promotion by comparing base-vs-head `trust` on the memory body.
//! Evidence is then verified by reading the structured
//! [`Transition::Trust`](ft_core::Transition) payload on the head record's
//! tail [`HistoryEntry`]: a promotion into [`TrustState::Verified`] must carry
//! `evidence_count > 0`. When the tail entry has no structured transition
//! (`None` — e.g. pre-existing history written before the field existed, or
//! entries merged across branches), we fall back to the legacy `evidence:`
//! substring marker in the entry's `ops_summary`.

use ft_core::{Record, RecordBody, RiskClass, Transition, TrustState};

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
        // Docs and repo profiles carry trust but no risk class, so the
        // high-stakes evidence rule never fires for them.
        RecordBody::Doc(b) => (b.trust, None),
        RecordBody::RepoProfile(b) => (b.trust, None),
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            return None;
        }
    })
}

/// Decide whether the head record carries recorded evidence for its promotion.
///
/// Reads the tail [`HistoryEntry`]'s structured transition:
/// - `Some(Transition::Trust { to: Verified, evidence_count, .. })` ⇒ the
///   transition is authoritative; require `evidence_count > 0`.
/// - `Some(Transition::Trust { .. })` with a non-`Verified` target ⇒ this tail
///   entry is not the promotion we care about; defer to the legacy marker.
/// - `None` ⇒ no structured transition (pre-existing / cross-branch-merged
///   history). Fall back to the legacy `evidence:` substring marker so older
///   histories still pass.
///
/// The legacy marker: `ft-cli` writes an `ops_summary` line containing the
/// case-insensitive substring `evidence:` when a reviewer attaches evidence.
fn has_evidence_marker(record: &Record) -> bool {
    let Some(tail) = record.envelope.history.last() else {
        return false;
    };
    match &tail.transition {
        Some(Transition::Trust {
            to: TrustState::Verified,
            evidence_count,
            ..
        }) => *evidence_count > 0,
        // A structured transition that isn't a promotion to Verified doesn't
        // describe this rule's trigger; fall back to the substring contract.
        Some(Transition::Trust { .. }) | None => {
            tail.ops_summary.iter().any(|s| has_evidence_prefix(s))
        }
    }
}

fn has_evidence_prefix(s: &str) -> bool {
    // History entries are prefixed with the kind tag (e.g. `update: `) by
    // `ft-history`. The evidence marker may appear at the start of the line
    // *or* after the kind tag. Match case-insensitive on the substring
    // `evidence:` to keep the convention permissive.
    let lower = s.to_ascii_lowercase();
    lower.contains("evidence:")
}
