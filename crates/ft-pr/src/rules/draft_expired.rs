//! Draft hygiene: memory records left in [`TrustState::Draft`] longer than
//! the configured expiry surface as warnings.

use chrono::Utc;
use ft_core::{Record, RecordBody, TrustState};

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    let now = Utc::now();
    for c in cx.changed {
        let Some(head) = c.at_head.as_ref() else {
            continue;
        };
        let Some(trust) = memory_trust(head) else {
            continue;
        };
        if trust != TrustState::Draft {
            continue;
        }
        let age = now.signed_duration_since(head.envelope.created_at);
        let days = age.num_days();
        if days > cx.opts.draft_max_age_days {
            report.push(PrFinding {
                severity: Severity::Warning,
                rule: RuleId::DraftExpired,
                record_id: Some(head.envelope.id.clone()),
                path: Some(c.path.clone()),
                message: format!(
                    "draft record is {} days old (expiry: {} days)",
                    days, cx.opts.draft_max_age_days
                ),
                details: serde_json::json!({
                    "age_days": days,
                    "expiry_days": cx.opts.draft_max_age_days,
                }),
            });
        }
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
