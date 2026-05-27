//! ADR-0010: tasks / subtasks / bugs that transition into `Closed` in this PR
//! must have every acceptance criterion in [`AcStatus::Checked`].

use ft_core::{AcStatus, AcceptanceCriterion, Record, RecordBody, Status};

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    for c in cx.changed {
        let Some(head) = c.at_head.as_ref() else {
            continue;
        };
        if head.envelope.status != Status::Closed {
            continue;
        }
        // Only fire when *this PR* is the one that closed the record; if it
        // was already closed at base, that was someone else's problem.
        if let Some(base) = c.at_base.as_ref() {
            if base.envelope.status == Status::Closed {
                continue;
            }
        }

        let unchecked = collect_unchecked(head);
        if unchecked.is_empty() {
            continue;
        }

        report.push(PrFinding {
            severity: Severity::Error,
            rule: RuleId::IncompleteAcceptance,
            record_id: Some(head.envelope.id.clone()),
            path: Some(c.path.clone()),
            message: format!(
                "closed record has {} unchecked acceptance criterion(s)",
                unchecked.len()
            ),
            details: serde_json::json!({
                "unchecked_ids": unchecked,
            }),
        });
    }
}

fn collect_unchecked(record: &Record) -> Vec<String> {
    let acs: &[AcceptanceCriterion] = match &record.body {
        RecordBody::Task(t) => &t.acceptance_criteria,
        RecordBody::Subtask(s) => &s.acceptance_criteria,
        RecordBody::Bug(b) => &b.acceptance_criteria,
        _ => return Vec::new(),
    };
    acs.iter()
        .filter(|a| a.status == AcStatus::Unchecked)
        .map(|a| a.id.clone())
        .collect()
}
