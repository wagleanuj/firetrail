//! Secret-scan: regex-based detection of API keys / tokens / credentials in
//! record bodies. Walks the head-side record as a JSON tree and checks every
//! string value against the configured pattern set, *skipping* fields that
//! are structurally hex-shaped (record ids, state hashes, history link
//! hashes). Those fields are not free-form content and would otherwise
//! produce false positives against the "32+ hex chars in quotes" pattern.

use regex::Regex;

use crate::report::{PrFinding, PrReport, RuleId, Severity};
use crate::validator::ValidationContext;

/// Field names whose string values are always structural hashes / ids and
/// must be excluded from secret scanning.
const SKIP_FIELDS: &[&str] = &[
    "state_hash",
    "prev_state_hash",
    "from_hash",
    "to_hash",
    "id",
    "content_hash",
    "commit_sha",
];

pub(crate) fn run(cx: &ValidationContext<'_>, report: &mut PrReport) {
    if !cx.opts.enable_secret_scan || cx.opts.secret_patterns.is_empty() {
        return;
    }
    for c in cx.changed {
        let Some(head) = c.at_head.as_ref() else {
            continue;
        };
        let Ok(value) = serde_json::to_value(head) else {
            continue;
        };
        let mut hit: Option<(String, &Regex)> = None;
        scan(&value, &cx.opts.secret_patterns, &mut hit);
        if let Some((matched, pat)) = hit {
            let snippet = redact(&matched);
            report.push(PrFinding {
                severity: Severity::Error,
                rule: RuleId::SecretLeak,
                record_id: Some(head.envelope.id.clone()),
                path: Some(c.path.clone()),
                message: format!("possible secret detected (pattern: `{}`)", pat.as_str()),
                details: serde_json::json!({
                    "pattern": pat.as_str(),
                    "match_preview": snippet,
                }),
            });
        }
    }
}

fn scan<'p>(
    value: &serde_json::Value,
    patterns: &'p [Regex],
    hit: &mut Option<(String, &'p Regex)>,
) {
    if hit.is_some() {
        return;
    }
    match value {
        serde_json::Value::String(s) => {
            for p in patterns {
                if let Some(m) = p.find(s) {
                    *hit = Some((m.as_str().to_string(), p));
                    return;
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                scan(v, patterns, hit);
                if hit.is_some() {
                    return;
                }
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if SKIP_FIELDS.contains(&k.as_str()) {
                    continue;
                }
                scan(v, patterns, hit);
                if hit.is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

/// Mask the middle of a matched secret so the report is safe to print and to
/// commit to CI logs.
fn redact(s: &str) -> String {
    let len = s.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    let prefix = &s[..4];
    let suffix = &s[len.saturating_sub(2)..];
    format!("{prefix}…{suffix}")
}
