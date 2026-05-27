//! ADR-0013: cap on acceptance criteria per record. Warning when exceeded.

use ft_core::{Record, RecordBody};

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    for c in cx.changed {
        let Some(head) = c.at_head.as_ref() else {
            continue;
        };
        let count = ac_count(head);
        if count > cx.opts.max_ac_per_record {
            report.push(PrFinding {
                severity: Severity::Warning,
                rule: RuleId::AcCapExceeded,
                record_id: Some(head.envelope.id.clone()),
                path: Some(c.path.clone()),
                message: format!(
                    "record has {} acceptance criteria (cap: {})",
                    count, cx.opts.max_ac_per_record
                ),
                details: serde_json::json!({
                    "count": count,
                    "cap": cx.opts.max_ac_per_record,
                }),
            });
        }
    }
}

fn ac_count(record: &Record) -> usize {
    match &record.body {
        RecordBody::Task(t) => t.acceptance_criteria.len(),
        RecordBody::Subtask(s) => s.acceptance_criteria.len(),
        RecordBody::Bug(b) => b.acceptance_criteria.len(),
        _ => 0,
    }
}
