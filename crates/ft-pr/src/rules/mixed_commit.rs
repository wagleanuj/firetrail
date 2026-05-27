//! ADR-0009: a single PR diff must not contain both code (non-record) paths
//! and memory-kind record paths.
//!
//! Structural records (Task/Epic/Subtask/Bug) co-commit with code freely;
//! the rule only fires when *memory-kind* records are mixed with code.

use ft_storage::ChangeClass;

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    let mut has_memory = false;
    let mut has_code = false;

    for c in cx.changed {
        match c.class {
            ChangeClass::Memory(_) => has_memory = true,
            ChangeClass::Other => has_code = true,
            // Structural records co-commit with code (ADR-0009).
            // Config files (.firetrail/scope.yaml, etc.) are infrastructure
            // and do not count as code for this rule.
            ChangeClass::Structural(_) | ChangeClass::Config => {}
        }
    }

    if has_memory && has_code {
        let mixed_paths: Vec<String> = cx
            .changed
            .iter()
            .filter(|c| matches!(c.class, ChangeClass::Memory(_) | ChangeClass::Other))
            .map(|c| c.path.display().to_string())
            .collect();
        report.push(PrFinding {
            severity: Severity::Error,
            rule: RuleId::MixedCommit,
            record_id: None,
            path: None,
            message: "PR mixes memory records with code changes (ADR-0009)".to_string(),
            details: serde_json::json!({ "paths": mixed_paths }),
        });
    }
}
