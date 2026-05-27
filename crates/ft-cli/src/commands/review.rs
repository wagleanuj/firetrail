//! `firetrail review <id>` — read-only interactive review helper.
//!
//! Renders the record envelope summary, current trust state + risk class,
//! acceptance criteria with status, attached evidence, history timeline
//! (with a chain-valid marker), and a suggested next action.

use ft_core::{AcStatus, Record, RecordBody, RiskClass, TrustState};
use ft_history::verify_chain;
use serde::Serialize;

use crate::cli::{GlobalOpts, ReviewArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "review";

/// Single acceptance-criterion row in the review output.
#[derive(Debug, Clone, Serialize)]
pub struct AcRow {
    /// Local id (`ac-01`, …).
    pub id: String,
    /// Criterion text.
    pub text: String,
    /// `checked` / `unchecked` (mirrors `AcStatus`).
    pub status: &'static str,
    /// Whether the AC was marked `proposed: true` (ADR-0013).
    pub proposed: bool,
    /// Attached evidence URL, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_url: Option<String>,
}

/// Single evidence row.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceRow {
    /// Local id (`ev-01`, …).
    pub id: String,
    /// Evidence kind.
    pub kind: String,
    /// Canonical URL.
    pub url: String,
    /// Free-form description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Single history-timeline entry.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryRow {
    /// 0-based index in the chain.
    pub index: usize,
    /// Number of operations compacted into this entry.
    pub ops_count: u32,
    /// PR number, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_via_pr: Option<u64>,
    /// RFC3339 timestamp.
    pub timestamp: String,
    /// Acting identity.
    pub actor: String,
    /// Compacted ops summary lines.
    pub ops_summary: Vec<String>,
    /// `to_hash` for this entry.
    pub to_hash: String,
}

/// `firetrail review`
pub fn run(args: &ReviewArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let id = ctx.resolve_id(&args.id)?;
    let record = ctx.read_record(&id)?;

    let trust = memory_trust(&record);
    let risk = memory_risk(&record);
    let acs = collect_acs(&record);
    let evidence = collect_evidence(&record);
    let history = collect_history(&record);

    let chain_ok = verify_chain(&record).is_ok();
    let chain_status = if chain_ok {
        "valid".to_string()
    } else {
        verify_chain(&record)
            .err()
            .map_or_else(|| "invalid".to_string(), |e| format!("invalid: {e}"))
    };

    let suggested = suggested_next_action(&record, trust, risk, &acs, &evidence, chain_ok);

    let outcome = ReviewOutcome {
        id: record.envelope.id.as_str().to_string(),
        kind: format!("{:?}", record.envelope.kind).to_ascii_lowercase(),
        title: record.envelope.title.clone(),
        status: format!("{:?}", record.envelope.status).to_ascii_lowercase(),
        priority: format!("{:?}", record.envelope.priority).to_ascii_lowercase(),
        owner: record
            .envelope
            .owner
            .as_ref()
            .map(|o| o.as_str().to_string()),
        owning_scope: record.envelope.owning_scope.clone(),
        created_by: record.envelope.created_by.as_str().to_string(),
        created_at: record.envelope.created_at.to_rfc3339(),
        updated_at: record.envelope.updated_at.to_rfc3339(),
        trust_state: trust.map(trust_label),
        risk_class: risk.map(risk_label),
        high_stakes: risk.is_some_and(RiskClass::is_high_stakes),
        state_hash: record.envelope.state_hash.clone(),
        chain_status,
        chain_valid: chain_ok,
        acceptance_criteria: acs,
        evidence,
        history,
        suggested_next_action: suggested,
        warnings,
    };
    Ok(CommandOutcome::Review(outcome))
}

/// Outcome of `firetrail review`.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewOutcome {
    /// Canonical id.
    pub id: String,
    /// Lowercased kind.
    pub kind: String,
    /// Title.
    pub title: String,
    /// Status (lowercase).
    pub status: String,
    /// Priority (lowercase).
    pub priority: String,
    /// Current owner identity, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Owning scope, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owning_scope: Option<String>,
    /// Creating identity.
    pub created_by: String,
    /// Creation timestamp (RFC3339).
    pub created_at: String,
    /// Updated timestamp (RFC3339).
    pub updated_at: String,
    /// Trust state (for memory kinds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_state: Option<&'static str>,
    /// Risk class (for memory kinds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_class: Option<&'static str>,
    /// `true` iff the risk class is high-stakes per ADR-0013.
    pub high_stakes: bool,
    /// On-disk envelope state hash.
    pub state_hash: String,
    /// `valid` or `invalid: <reason>`.
    pub chain_status: String,
    /// Convenience boolean mirroring `chain_status`.
    pub chain_valid: bool,
    /// Acceptance criteria (empty for kinds that don't have them).
    pub acceptance_criteria: Vec<AcRow>,
    /// Attached evidence.
    pub evidence: Vec<EvidenceRow>,
    /// History timeline.
    pub history: Vec<HistoryRow>,
    /// Suggested next action (free-form, advisory only).
    pub suggested_next_action: String,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ReviewOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# review `{}`\n\n**{}**\n\n- kind: `{}`\n- status: `{}`\n- priority: `{}`\n",
            self.id, self.title, self.kind, self.status, self.priority
        );
        if let Some(o) = &self.owner {
            let _ = writeln!(s, "- owner: `{o}`");
        }
        if let Some(scope) = &self.owning_scope {
            let _ = writeln!(s, "- scope: `{scope}`");
        }
        let _ = writeln!(s, "- created_by: `{}`", self.created_by);
        let _ = writeln!(s, "- created_at: {}", self.created_at);
        let _ = writeln!(s, "- updated_at: {}", self.updated_at);
        if let Some(trust) = self.trust_state {
            let _ = writeln!(s, "- trust: `{trust}`");
        }
        if let Some(risk) = self.risk_class {
            let _ = writeln!(
                s,
                "- risk: `{risk}`{}",
                if self.high_stakes {
                    " (high-stakes)"
                } else {
                    ""
                }
            );
        }
        let _ = writeln!(s, "- state_hash: `{}`", self.state_hash);
        let _ = writeln!(s, "- chain: `{}`", self.chain_status);

        if !self.acceptance_criteria.is_empty() {
            s.push_str("\n## Acceptance criteria\n\n");
            for ac in &self.acceptance_criteria {
                let mark = if ac.status == "checked" { "x" } else { " " };
                let _ = writeln!(
                    s,
                    "- [{mark}] `{}` {} {}",
                    ac.id,
                    ac.text,
                    if ac.proposed { "(proposed)" } else { "" }
                );
                if let Some(url) = &ac.evidence_url {
                    let _ = writeln!(s, "    - evidence: {url}");
                }
            }
        }

        if !self.evidence.is_empty() {
            s.push_str("\n## Evidence\n\n");
            for ev in &self.evidence {
                let _ = writeln!(s, "- `{}` ({}): {}", ev.id, ev.kind, ev.url);
            }
        }

        if !self.history.is_empty() {
            s.push_str("\n## History\n\n");
            for h in &self.history {
                let _ = writeln!(
                    s,
                    "- [{}] {} — {} op(s) by `{}`",
                    h.index, h.timestamp, h.ops_count, h.actor
                );
                for op in &h.ops_summary {
                    let _ = writeln!(s, "    - {op}");
                }
            }
        }

        let _ = writeln!(
            s,
            "\n## Suggested next action\n\n{}",
            self.suggested_next_action
        );
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!("review {}: {}", self.id, self.suggested_next_action)
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

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

fn memory_risk(record: &Record) -> Option<RiskClass> {
    match &record.body {
        RecordBody::Incident(b) => b.risk_class,
        RecordBody::Finding(b) => b.risk_class,
        RecordBody::Runbook(b) => b.risk_class,
        RecordBody::Decision(b) => b.risk_class,
        RecordBody::Gotcha(b) => b.risk_class,
        RecordBody::Memory(b) => b.risk_class,
        _ => None,
    }
}

fn trust_label(t: TrustState) -> &'static str {
    match t {
        TrustState::Draft => "draft",
        TrustState::Reviewed => "reviewed",
        TrustState::Verified => "verified",
        TrustState::Stale => "stale",
        TrustState::Deprecated => "deprecated",
        TrustState::Archived => "archived",
        TrustState::Superseded => "superseded",
        TrustState::Rejected => "rejected",
        TrustState::Redacted => "redacted",
    }
}

fn risk_label(r: RiskClass) -> &'static str {
    match r {
        RiskClass::Security => "security",
        RiskClass::Availability => "availability",
        RiskClass::DataLoss => "data_loss",
        RiskClass::Compliance => "compliance",
        RiskClass::Performance => "performance",
        RiskClass::Correctness => "correctness",
    }
}

fn collect_acs(record: &Record) -> Vec<AcRow> {
    let acs: &[ft_core::AcceptanceCriterion] = match &record.body {
        RecordBody::Task(t) => &t.acceptance_criteria,
        RecordBody::Subtask(s) => &s.acceptance_criteria,
        RecordBody::Bug(b) => &b.acceptance_criteria,
        _ => return Vec::new(),
    };
    acs.iter()
        .map(|a| AcRow {
            id: a.id.clone(),
            text: a.text.clone(),
            status: match a.status {
                AcStatus::Checked => "checked",
                AcStatus::Unchecked => "unchecked",
            },
            proposed: a.proposed,
            evidence_url: a.evidence_url.clone(),
        })
        .collect()
}

fn collect_evidence(record: &Record) -> Vec<EvidenceRow> {
    let evs: &[ft_core::Evidence] = match &record.body {
        RecordBody::Task(t) => &t.evidence,
        RecordBody::Subtask(s) => &s.evidence,
        RecordBody::Bug(b) => &b.evidence,
        _ => return Vec::new(),
    };
    evs.iter()
        .map(|e| EvidenceRow {
            id: e.id.clone(),
            kind: serde_json::to_value(e.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:?}", e.kind)),
            url: e.url.clone(),
            description: e.description.clone(),
        })
        .collect()
}

fn collect_history(record: &Record) -> Vec<HistoryRow> {
    record
        .envelope
        .history
        .iter()
        .enumerate()
        .map(|(i, h)| HistoryRow {
            index: i,
            ops_count: h.ops_count,
            merged_via_pr: h.merged_via_pr,
            timestamp: h.timestamp.to_rfc3339(),
            actor: h.primary_actor.as_str().to_string(),
            ops_summary: h.ops_summary.clone(),
            to_hash: h.to_hash.clone(),
        })
        .collect()
}

fn suggested_next_action(
    record: &Record,
    trust: Option<TrustState>,
    risk: Option<RiskClass>,
    acs: &[AcRow],
    evidence: &[EvidenceRow],
    chain_ok: bool,
) -> String {
    if !chain_ok {
        return "chain integrity broken — run `firetrail verify` and investigate before further edits".to_string();
    }
    // High-stakes Reviewed without evidence — recommend promotion path.
    if let (Some(TrustState::Reviewed), Some(r)) = (trust, risk) {
        if r.is_high_stakes() && evidence.is_empty() {
            return format!(
                "promote with `firetrail memory promote {} --evidence-url <url> --evidence-type pull_request` (high-stakes record requires evidence per ADR-0013)",
                record.envelope.id.as_str()
            );
        }
    }
    if let Some(TrustState::Draft) = trust {
        return format!(
            "review with `firetrail memory review {}` once the draft is ready",
            record.envelope.id.as_str()
        );
    }
    if !acs.is_empty() {
        let unchecked = acs.iter().filter(|a| a.status == "unchecked").count();
        if unchecked > 0 {
            return format!(
                "{unchecked} acceptance criteria still unchecked — work through them or close with `firetrail close --force --reason ...`"
            );
        }
    }
    "no action needed — record looks healthy".to_string()
}
