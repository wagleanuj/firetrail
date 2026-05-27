//! ADR-0010: warn when a commit message in `base..head` claims to close a
//! record (via `firetrail-closes:` / `Closes #N` footer) but the referenced
//! record cannot be resolved to a matching closure in this PR.
//!
//! Implementation note: `ft-git::Repo` does not currently expose a "log of
//! commit messages between two refs" call, so this rule shells out to
//! `git log --format=%B base..head` against the repo root. Failure to shell
//! out is treated as "no claims found" rather than an error so the rule does
//! not break CI on shallow clones.

use std::process::Command;

use ft_core::{RecordId, Status};
use regex::Regex;

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    let Some(messages) = collect_commit_messages(cx) else {
        return;
    };

    let footer_re = Regex::new(r"(?i)\bfiretrail-closes:\s*([A-Z]+-[0-9a-fA-F]+)").ok();
    let fixes_re = Regex::new(r"(?i)\b(?:closes|fixes|resolves):?\s+([A-Z]+-[0-9a-fA-F]+)").ok();

    let mut claims: Vec<RecordId> = Vec::new();
    for msg in &messages {
        if let Some(re) = &footer_re {
            for cap in re.captures_iter(msg) {
                if let Some(m) = cap.get(1) {
                    if let Ok(id) = RecordId::from_string(m.as_str().to_string()) {
                        claims.push(id);
                    }
                }
            }
        }
        if let Some(re) = &fixes_re {
            for cap in re.captures_iter(msg) {
                if let Some(m) = cap.get(1) {
                    if let Ok(id) = RecordId::from_string(m.as_str().to_string()) {
                        claims.push(id);
                    }
                }
            }
        }
    }
    claims.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    claims.dedup();

    for claim in claims {
        let in_diff = cx
            .by_id
            .get(&claim)
            .and_then(|idx| cx.changed[*idx].at_head.as_ref());
        let resolved = in_diff.is_some_and(|r| r.envelope.status == Status::Closed);
        if !resolved {
            report.push(PrFinding {
                severity: Severity::Warning,
                rule: RuleId::PrLinkMissing,
                record_id: Some(claim.clone()),
                path: None,
                message: format!(
                    "commit claims to close {} but no matching closure was found in this PR",
                    claim.as_str()
                ),
                details: serde_json::json!({ "claim": claim }),
            });
        }
    }
}

fn collect_commit_messages(cx: &ValidationContext<'_>) -> Option<Vec<String>> {
    let range = format!("{}..{}", cx.base, cx.head);
    let output = Command::new("git")
        .args(["log", "--format=%B%x1e", &range])
        .current_dir(cx.git.root())
        .output()
        .ok()?;
    if !output.status.success() {
        return Some(Vec::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    Some(
        raw.split('\u{1e}')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}
