//! ADR-0017: per-record `state_hash` chain integrity. Any record in the diff
//! that fails [`ft_history::verify_chain`] yields an Error finding.

use ft_history::verify_chain;

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    for c in cx.changed {
        let Some(record) = c.at_head.as_ref() else {
            continue;
        };
        if let Err(e) = verify_chain(record) {
            report.push(PrFinding {
                severity: Severity::Error,
                rule: RuleId::ChainBroken,
                record_id: Some(record.envelope.id.clone()),
                path: Some(c.path.clone()),
                message: format!("chain integrity failure: {e}"),
                details: serde_json::json!({ "error": e.to_string() }),
            });
        }
    }
}
