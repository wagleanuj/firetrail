//! Warn when a record references a target that is currently in one of the
//! "do not consume" states: `Deprecated`, `Archived`, `Rejected`, `Redacted`
//! (memory-kind trust states) or [`Status::Archived`] (envelope status).
//!
//! References examined:
//!
//! - `Task.parent_epic`
//! - `Subtask.parent_task`
//! - `Incident.findings`, `Incident.runbooks_invoked`
//! - `Finding.incident`, `Finding.superseded_by`
//! - `Decision.superseded_by`
//! - `Memory.related`
//!
//! For each referenced id we ask the storage layer for the record at `head`.
//! If the target is in a deprecated-style state, the rule fires.

use ft_core::{Record, RecordBody, RecordId, Status, TrustState};

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    for c in cx.changed {
        let Some(head) = c.at_head.as_ref() else {
            continue;
        };
        let refs = collect_references(head);
        for r in refs {
            // Skip if the reference points at a record also in this PR (covered
            // by other rules) and the in-diff version is fine.
            let Some(target) = resolve_reference(cx, &r) else {
                // Missing references are not this rule's concern.
                continue;
            };
            if let Some(state) = deprecated_state(&target) {
                report.push(PrFinding {
                    severity: Severity::Warning,
                    rule: RuleId::DeprecatedReference,
                    record_id: Some(head.envelope.id.clone()),
                    path: Some(c.path.clone()),
                    message: format!(
                        "references {} which is in state `{}`",
                        target.envelope.id.as_str(),
                        state
                    ),
                    details: serde_json::json!({
                        "target_id": target.envelope.id,
                        "target_state": state,
                    }),
                });
            }
        }
    }
}

fn collect_references(record: &Record) -> Vec<RecordId> {
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

fn resolve_reference(cx: &ValidationContext<'_>, id: &RecordId) -> Option<Record> {
    // Prefer in-PR (head) view.
    if let Some(idx) = cx.by_id.get(id) {
        if let Some(r) = cx.changed[*idx].at_head.as_ref() {
            return Some(r.clone());
        }
    }
    // Fall back to storage at head.
    cx.storage.read_at_ref(cx.head, id).ok()
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
