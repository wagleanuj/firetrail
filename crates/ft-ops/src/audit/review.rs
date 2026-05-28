//! `review` op — read-only review summary for a single record.
//!
//! Mirrors `ft_cli::commands::review`. The CLI prompts approve/reject in a
//! human-readable form; here we expose only the structured summary. An
//! actual approve/reject workflow (with [`crate::Event::ReviewApproved`] /
//! [`crate::Event::ReviewRejected`]) is reserved for a follow-up wave; the
//! input shape is left forward-compatible.

use ft_core::{AcStatus, Record, RecordBody, RiskClass, TrustState};
use ft_history::verify_chain;
use ft_storage::{EmbeddedStorage, Storage as _, StorageFilter};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

/// One acceptance-criterion row.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewAcRow {
    /// Local id.
    pub id: String,
    /// Criterion text.
    pub text: String,
    /// `checked` / `unchecked`.
    pub status: String,
    /// Whether the AC was marked proposed.
    pub proposed: bool,
    /// Attached evidence URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_url: Option<String>,
}

/// One evidence row.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEvidenceRow {
    /// Local id.
    pub id: String,
    /// Evidence kind.
    pub kind: String,
    /// URL.
    pub url: String,
    /// Free-form description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One history row.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHistoryRow {
    /// 0-based index in the chain.
    pub index: usize,
    /// Number of ops compacted into this entry.
    pub ops_count: u32,
    /// PR number, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// Input for [`review`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "AuditReviewInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewInput {
    /// Record id (full canonical or unambiguous prefix).
    pub id: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`review`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "AuditReviewOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOutput {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Owning scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_scope: Option<String>,
    /// Creating identity.
    pub created_by: String,
    /// Creation timestamp (RFC3339).
    pub created_at: String,
    /// Updated timestamp (RFC3339).
    pub updated_at: String,
    /// Trust state, for memory kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_state: Option<String>,
    /// Risk class, for memory kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_class: Option<String>,
    /// `true` iff the risk class is high-stakes (ADR-0013).
    pub high_stakes: bool,
    /// On-disk state hash.
    pub state_hash: String,
    /// `valid` or `invalid: <reason>`.
    pub chain_status: String,
    /// Convenience boolean.
    pub chain_valid: bool,
    /// Acceptance criteria.
    pub acceptance_criteria: Vec<ReviewAcRow>,
    /// Attached evidence.
    pub evidence: Vec<ReviewEvidenceRow>,
    /// History timeline.
    pub history: Vec<ReviewHistoryRow>,
    /// Free-form suggested next action.
    pub suggested_next_action: String,
}

/// `review` op.
#[allow(clippy::needless_pass_by_value)]
pub fn review(
    ws: &Workspace,
    _identity: &Identity,
    input: ReviewInput,
    _events: &EventBus,
) -> Result<ReviewOutput, OpsError> {
    let storage = EmbeddedStorage::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open storage: {e}")))?;
    let id = resolve_id(&storage, &input.id)?;
    let record = storage.read(&id).map_err(|e| match e {
        ft_storage::StorageError::NotFound(_) => {
            OpsError::not_found("memory", id.as_str().to_string())
        }
        other => OpsError::Internal(anyhow::anyhow!("read record: {other}")),
    })?;

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

    Ok(ReviewOutput {
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
        trust_state: trust.map(|t| trust_label(t).to_string()),
        risk_class: risk.map(|r| risk_label(r).to_string()),
        high_stakes: risk.is_some_and(RiskClass::is_high_stakes),
        state_hash: record.envelope.state_hash.clone(),
        chain_status,
        chain_valid: chain_ok,
        acceptance_criteria: acs,
        evidence,
        history,
        suggested_next_action: suggested,
    })
}

fn resolve_id(storage: &EmbeddedStorage, raw: &str) -> Result<ft_core::RecordId, OpsError> {
    if let Ok(id) = ft_core::RecordId::from_string(raw.to_string()) {
        return if storage.read(&id).is_ok() {
            Ok(id)
        } else {
            Err(OpsError::not_found("memory", raw.to_string()))
        };
    }
    let candidates = storage
        .list(&StorageFilter::default())
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("scan storage: {e}")))?;
    match ft_core::resolve_prefix(raw, &candidates) {
        Ok(id) => Ok(id),
        Err(ft_core::ResolveError::Empty) => Err(OpsError::validation("id", "empty record id")),
        Err(ft_core::ResolveError::EmptyHexPrefix(k)) => Err(OpsError::validation(
            "id",
            format!("hex prefix required after kind tag `{k}`"),
        )),
        Err(ft_core::ResolveError::Unknown(_)) => {
            Err(OpsError::not_found("memory", raw.to_string()))
        }
        Err(ft_core::ResolveError::Ambiguous { matches, .. }) => Err(OpsError::Conflict {
            reason: format!("`{raw}` is ambiguous; matches {} records", matches.len()),
        }),
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

fn collect_acs(record: &Record) -> Vec<ReviewAcRow> {
    let acs: &[ft_core::AcceptanceCriterion] = match &record.body {
        RecordBody::Task(t) => &t.acceptance_criteria,
        RecordBody::Subtask(s) => &s.acceptance_criteria,
        RecordBody::Bug(b) => &b.acceptance_criteria,
        _ => return Vec::new(),
    };
    acs.iter()
        .map(|a| ReviewAcRow {
            id: a.id.clone(),
            text: a.text.clone(),
            status: match a.status {
                AcStatus::Checked => "checked".into(),
                AcStatus::Unchecked => "unchecked".into(),
            },
            proposed: a.proposed,
            evidence_url: a.evidence_url.clone(),
        })
        .collect()
}

fn collect_evidence(record: &Record) -> Vec<ReviewEvidenceRow> {
    let evs: &[ft_core::Evidence] = match &record.body {
        RecordBody::Task(t) => &t.evidence,
        RecordBody::Subtask(s) => &s.evidence,
        RecordBody::Bug(b) => &b.evidence,
        _ => return Vec::new(),
    };
    evs.iter()
        .map(|e| ReviewEvidenceRow {
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

fn collect_history(record: &Record) -> Vec<ReviewHistoryRow> {
    record
        .envelope
        .history
        .iter()
        .enumerate()
        .map(|(i, h)| ReviewHistoryRow {
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
    acs: &[ReviewAcRow],
    evidence: &[ReviewEvidenceRow],
    chain_ok: bool,
) -> String {
    if !chain_ok {
        return "chain integrity broken — run `firetrail verify` and investigate before further edits".into();
    }
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
    "no action needed — record looks healthy".into()
}
