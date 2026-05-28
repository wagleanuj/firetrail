//! Trust transition ops over memory records.
//!
//! Every op follows the same shape: open a [`super::super::memory::ctx::MemoryCtx`],
//! resolve the record, validate the transition via `ft_trust`, then persist
//! via `save_record_with_history` so the chain entry is appended atomically
//! with the body mutation.

use chrono::Utc;
use ft_core::{Evidence, EvidenceKind, Identity as CoreIdentity, Record, RecordId, TrustState};
use ft_history::{HistoryDraft, HistoryEntryKind};
use ft_trust::{MemoryBody, TrustTransition, apply_transition, validate_transition};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::memory::ctx_for_trust;
use crate::workspace::Workspace;

/// Evidence kind on a trust transition input.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKindInput {
    /// Incident report.
    IncidentReport,
    /// Pull request.
    PullRequest,
    /// Commit.
    Commit,
    /// Dashboard.
    Dashboard,
    /// Log query.
    LogQuery,
    /// Test result.
    TestResult,
    /// Jira ticket.
    JiraTicket,
    /// Confluence page.
    ConfluencePage,
    /// Manual note.
    ManualNote,
}

impl EvidenceKindInput {
    fn to_core(self) -> EvidenceKind {
        match self {
            Self::IncidentReport => EvidenceKind::IncidentReport,
            Self::PullRequest => EvidenceKind::PullRequest,
            Self::Commit => EvidenceKind::Commit,
            Self::Dashboard => EvidenceKind::Dashboard,
            Self::LogQuery => EvidenceKind::LogQuery,
            Self::TestResult => EvidenceKind::TestResult,
            Self::JiraTicket => EvidenceKind::JiraTicket,
            Self::ConfluencePage => EvidenceKind::ConfluencePage,
            Self::ManualNote => EvidenceKind::ManualNote,
        }
    }
}

fn evidence_kind_tag(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::IncidentReport => "incident-report",
        EvidenceKind::PullRequest => "pull-request",
        EvidenceKind::Commit => "commit",
        EvidenceKind::Dashboard => "dashboard",
        EvidenceKind::LogQuery => "log-query",
        EvidenceKind::TestResult => "test-result",
        EvidenceKind::JiraTicket => "jira-ticket",
        EvidenceKind::ConfluencePage => "confluence-page",
        EvidenceKind::ManualNote => "manual-note",
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

/// Shared output: the updated record after a trust transition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustOutput {
    /// The mutated record (envelope + body).
    pub record: Record,
}

// ─────────────────────────────────────────────────────────────────────────────
// Inputs
// ─────────────────────────────────────────────────────────────────────────────

/// Input for [`review`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "TrustReviewInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewInput {
    /// Memory id (full canonical or unambiguous prefix).
    pub id: String,
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional evidence URL.
    #[serde(default)]
    pub evidence_url: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`promote`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "TrustPromoteInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteInput {
    /// Memory id.
    pub id: String,
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
    /// Evidence URL (required for high-stakes risk classes per ADR-0013).
    #[serde(default)]
    pub evidence_url: Option<String>,
    /// Evidence kind.
    #[serde(default)]
    pub evidence_type: Option<EvidenceKindInput>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`deprecate`] / [`redact`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "TrustReasonInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonInput {
    /// Memory id.
    pub id: String,
    /// Reason text (required).
    pub reason: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`archive`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "TrustArchiveInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInput {
    /// Memory id.
    pub id: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`supersede`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "TrustSupersedeInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupersedeInput {
    /// Memory id being superseded.
    pub id: String,
    /// Successor memory id.
    pub successor: String,
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`merge`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "TrustMergeInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeInput {
    /// Canonical id (every `others` entry is superseded by this id).
    pub canonical: String,
    /// Ids to merge into `canonical`.
    pub others: Vec<String>,
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`merge`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeOutput {
    /// Final state of the last-superseded record (informational).
    pub last_record: Record,
    /// Number of records that were superseded.
    pub count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Ops
// ─────────────────────────────────────────────────────────────────────────────

/// `memory review` op (Draft → Reviewed).
pub fn review(
    ws: &Workspace,
    identity: &Identity,
    input: ReviewInput,
    events: &EventBus,
) -> Result<TrustOutput, OpsError> {
    let mut ctx = ctx_for_trust(ws, identity, "memory review")?;
    let actor = ctx.actor.clone();
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let evidence = build_evidence_url(&actor, input.evidence_url.as_deref(), None);
    let transition = build_transition(
        &record,
        TrustState::Reviewed,
        actor.clone(),
        input.reason,
        evidence,
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
    emit_transition(
        events,
        input.request_id.as_deref(),
        &id,
        transition.from,
        transition.to,
    );
    Ok(TrustOutput { record })
}

/// `memory promote` op (Reviewed → Verified).
pub fn promote(
    ws: &Workspace,
    identity: &Identity,
    input: PromoteInput,
    events: &EventBus,
) -> Result<TrustOutput, OpsError> {
    let mut ctx = ctx_for_trust(ws, identity, "memory promote")?;
    let actor = ctx.actor.clone();
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let evidence_kind = input.evidence_type.map(EvidenceKindInput::to_core);
    let evidence = build_evidence_url(&actor, input.evidence_url.as_deref(), evidence_kind);
    let transition = build_transition(
        &record,
        TrustState::Verified,
        actor.clone(),
        input.reason,
        evidence,
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
    emit_transition(
        events,
        input.request_id.as_deref(),
        &id,
        transition.from,
        transition.to,
    );
    Ok(TrustOutput { record })
}

/// `memory deprecate` op.
pub fn deprecate(
    ws: &Workspace,
    identity: &Identity,
    input: ReasonInput,
    events: &EventBus,
) -> Result<TrustOutput, OpsError> {
    let mut ctx = ctx_for_trust(ws, identity, "memory deprecate")?;
    let actor = ctx.actor.clone();
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Deprecated,
        actor.clone(),
        Some(input.reason),
        Vec::new(),
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
    emit_transition(
        events,
        input.request_id.as_deref(),
        &id,
        transition.from,
        transition.to,
    );
    Ok(TrustOutput { record })
}

/// `memory archive` op.
#[allow(clippy::needless_pass_by_value)]
pub fn archive(
    ws: &Workspace,
    identity: &Identity,
    input: ArchiveInput,
    events: &EventBus,
) -> Result<TrustOutput, OpsError> {
    let mut ctx = ctx_for_trust(ws, identity, "memory archive")?;
    let actor = ctx.actor.clone();
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Archived,
        actor.clone(),
        None,
        Vec::new(),
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
    emit_transition(
        events,
        input.request_id.as_deref(),
        &id,
        transition.from,
        transition.to,
    );
    Ok(TrustOutput { record })
}

/// `memory supersede` op.
pub fn supersede(
    ws: &Workspace,
    identity: &Identity,
    input: SupersedeInput,
    events: &EventBus,
) -> Result<TrustOutput, OpsError> {
    let mut ctx = ctx_for_trust(ws, identity, "memory supersede")?;
    let actor = ctx.actor.clone();
    let id = ctx.resolve_id(&input.id)?;
    let successor = ctx.resolve_id(&input.successor)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Superseded,
        actor.clone(),
        input.reason,
        Vec::new(),
        Some(successor),
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
    emit_transition(
        events,
        input.request_id.as_deref(),
        &id,
        transition.from,
        transition.to,
    );
    Ok(TrustOutput { record })
}

/// `memory merge` op — supersede every "other" id with `canonical`.
#[allow(clippy::needless_pass_by_value)]
pub fn merge(
    ws: &Workspace,
    identity: &Identity,
    input: MergeInput,
    events: &EventBus,
) -> Result<MergeOutput, OpsError> {
    if input.others.is_empty() {
        return Err(OpsError::validation(
            "others",
            "merge requires at least one other record id",
        ));
    }
    let mut ctx = ctx_for_trust(ws, identity, "memory merge")?;
    let actor = ctx.actor.clone();
    let canonical = ctx.resolve_id(&input.canonical)?;
    let mut last_record: Option<Record> = None;
    let mut count = 0usize;
    for raw in &input.others {
        let other_id = ctx.resolve_id(raw)?;
        if other_id == canonical {
            return Err(OpsError::validation(
                "others",
                format!("cannot merge {other_id} into itself"),
            ));
        }
        let mut record = ctx.read_record(&other_id)?;
        let transition = build_transition(
            &record,
            TrustState::Superseded,
            actor.clone(),
            input
                .reason
                .clone()
                .or_else(|| Some(format!("merged into {canonical}"))),
            Vec::new(),
            Some(canonical.clone()),
        )?;
        apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
        emit_transition(
            events,
            input.request_id.as_deref(),
            &other_id,
            transition.from,
            transition.to,
        );
        last_record = Some(record);
        count += 1;
    }
    Ok(MergeOutput {
        last_record: last_record.expect("validated non-empty above"),
        count,
    })
}

/// `memory redact` op.
pub fn redact(
    ws: &Workspace,
    identity: &Identity,
    input: ReasonInput,
    events: &EventBus,
) -> Result<TrustOutput, OpsError> {
    let mut ctx = ctx_for_trust(ws, identity, "memory redact")?;
    let actor = ctx.actor.clone();
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Redacted,
        actor.clone(),
        Some(input.reason),
        Vec::new(),
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition)?;
    emit_transition(
        events,
        input.request_id.as_deref(),
        &id,
        transition.from,
        transition.to,
    );
    Ok(TrustOutput { record })
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers (mirroring ft_cli::commands::trust)
// ─────────────────────────────────────────────────────────────────────────────

fn prior_reviewers(record: &Record) -> Vec<CoreIdentity> {
    let mut out: Vec<CoreIdentity> = Vec::new();
    for entry in &record.envelope.history {
        let head = entry.ops_summary.first().map_or("", String::as_str);
        let tag = head.split_once(':').map_or("", |(t, _)| t.trim());
        if HistoryEntryKind::from_tag(tag) == Some(HistoryEntryKind::TrustTransition)
            && !out.contains(&entry.primary_actor)
        {
            out.push(entry.primary_actor.clone());
        }
    }
    out
}

fn current_trust(
    record: &mut Record,
) -> Result<(TrustState, Option<ft_core::RiskClass>), OpsError> {
    let view = MemoryBody::from_record_body(&mut record.body)
        .map_err(|e| OpsError::validation("record", format!("not a memory record: {e}")))?;
    Ok((view.trust(), view.risk_class()))
}

fn build_transition(
    record: &Record,
    to: TrustState,
    reviewer: CoreIdentity,
    reason: Option<String>,
    evidence: Vec<Evidence>,
    successor: Option<RecordId>,
) -> Result<TrustTransition, OpsError> {
    let mut mutable = record.clone();
    let (from, _risk) = current_trust(&mut mutable)?;
    Ok(TrustTransition {
        from,
        to,
        reviewer,
        origin: ft_core::Origin::Human,
        reason,
        evidence,
        successor,
        occurred_at: Utc::now(),
    })
}

fn apply_and_persist(
    ctx: &mut crate::memory::ctx::MemoryCtx<'_>,
    record: &mut Record,
    actor: &CoreIdentity,
    transition: &TrustTransition,
) -> Result<(), OpsError> {
    let prior = prior_reviewers(record);
    let (current_state, risk) = current_trust(record)?;
    if transition.from != current_state {
        return Err(OpsError::conflict(format!(
            "record is in {:?}, refusing transition that expects {:?}",
            current_state, transition.from
        )));
    }
    let author = record.envelope.created_by.clone();
    validate_transition(current_state, risk, transition, &prior, &author)
        .map_err(|e| OpsError::validation("transition", e.to_string()))?;

    let applied = {
        let mut view = MemoryBody::from_record_body(&mut record.body)
            .map_err(|e| OpsError::validation("record", format!("not a memory record: {e}")))?;
        apply_transition(&mut view, transition)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("apply: {e}")))?
    };
    record.envelope.updated_at = applied.occurred_at;

    let summary = format!(
        "{:?}→{:?}{}",
        applied.from,
        applied.to,
        applied
            .reason
            .as_deref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default(),
    );
    let mut ops_summary = vec![summary];
    for ev in &applied.evidence {
        ops_summary.push(format!(
            "evidence: type={} url={}",
            evidence_kind_tag(ev.kind),
            ev.url
        ));
    }
    let draft = HistoryDraft {
        merged_via_pr: None,
        timestamp: applied.occurred_at,
        primary_actor: actor.clone(),
        contributors: Vec::new(),
        ops_summary,
        ops_count: 1,
        kind: HistoryEntryKind::TrustTransition,
    };
    ctx.save_record_with_history(record, draft)?;
    Ok(())
}

fn build_evidence_url(
    actor: &CoreIdentity,
    url: Option<&str>,
    kind: Option<EvidenceKind>,
) -> Vec<Evidence> {
    let Some(url) = url else { return Vec::new() };
    vec![Evidence {
        id: "ev-ops".to_string(),
        kind: kind.unwrap_or(EvidenceKind::ManualNote),
        url: url.to_string(),
        description: None,
        created_at: Utc::now(),
        created_by: actor.clone(),
        commit_sha: None,
        symbol_name: None,
        content_hash: None,
    }]
}

fn emit_transition(
    bus: &EventBus,
    request_id: Option<&str>,
    id: &RecordId,
    from: TrustState,
    to: TrustState,
) {
    let event = Event::TrustTransitioned {
        id: id.as_str().to_string(),
        from: trust_label(from).to_string(),
        to: trust_label(to).to_string(),
    };
    if let Some(rid) = request_id {
        bus.emit_with_request(rid.to_string(), event);
    } else {
        bus.emit(event);
    }
}
