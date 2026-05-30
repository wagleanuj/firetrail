//! Memory-record creation ops.
//!
//! One op per memory kind, plus a polymorphic [`capture`] op that mirrors
//! `firetrail capture` (the body comes in as a typed field rather than via
//! stdin — the CLI fills it from `--body` or stdin before calling ops).
//!
//! Every successful op emits [`crate::Event::MemoryCreated`] (Wave 2-A
//! semantics: new record, distinct from update-side `MemoryWritten`).

use chrono::{DateTime, Utc};
use ft_core::{
    Decision, DecisionStatus, Finding, Gotcha, Incident, Memory, Origin, Record, RecordBuilder,
    RecordKind, RiskClass, Runbook, Severity, TrustState,
};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::MemoryCtx;

// ─────────────────────────────────────────────────────────────────────────────
// Wire-friendly enums (severity, risk class, memory kind).
// ─────────────────────────────────────────────────────────────────────────────

/// Severity selector for [`create_incident`] and [`capture`] (incident form).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SeverityInput {
    /// `sev1` — customer-impacting outage.
    Sev1,
    /// `sev2` — major degradation.
    Sev2,
    /// `sev3` — minor impact.
    Sev3,
    /// `sev4` — informational.
    Sev4,
}

impl SeverityInput {
    fn to_core(self) -> Severity {
        match self {
            Self::Sev1 => Severity::Sev1,
            Self::Sev2 => Severity::Sev2,
            Self::Sev3 => Severity::Sev3,
            Self::Sev4 => Severity::Sev4,
        }
    }
}

/// Risk-class selector (ADR-0013).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RiskClassInput {
    /// Security risk.
    Security,
    /// Availability risk.
    Availability,
    /// Data-loss risk.
    DataLoss,
    /// Compliance risk.
    Compliance,
    /// Performance risk.
    Performance,
    /// Correctness risk.
    Correctness,
}

impl RiskClassInput {
    pub(crate) fn to_core(self) -> RiskClass {
        match self {
            Self::Security => RiskClass::Security,
            Self::Availability => RiskClass::Availability,
            Self::DataLoss => RiskClass::DataLoss,
            Self::Compliance => RiskClass::Compliance,
            Self::Performance => RiskClass::Performance,
            Self::Correctness => RiskClass::Correctness,
        }
    }
}

/// Lifecycle-status selector for [`create_decision`].
///
/// Mirrors [`ft_core::DecisionStatus`]; kept separate so the wire surface
/// stays decoupled from the core enum (same pattern as [`SeverityInput`] /
/// [`RiskClassInput`]).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatusInput {
    /// Drafted and under discussion.
    Proposed,
    /// Accepted and current.
    Accepted,
    /// Replaced by a successor decision.
    Superseded,
    /// No longer applicable but kept for audit.
    Deprecated,
}

impl DecisionStatusInput {
    fn to_core(self) -> DecisionStatus {
        match self {
            Self::Proposed => DecisionStatus::Proposed,
            Self::Accepted => DecisionStatus::Accepted,
            Self::Superseded => DecisionStatus::Superseded,
            Self::Deprecated => DecisionStatus::Deprecated,
        }
    }
}

/// Memory-kind selector for [`capture`] and `memory list --kind`.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    /// Incident record.
    Incident,
    /// Finding record.
    Finding,
    /// Runbook record.
    Runbook,
    /// Decision record.
    Decision,
    /// Gotcha record.
    Gotcha,
    /// Generic memory note.
    Memory,
}

impl MemoryKind {
    pub(crate) fn to_core(self) -> RecordKind {
        match self {
            Self::Incident => RecordKind::Incident,
            Self::Finding => RecordKind::Finding,
            Self::Runbook => RecordKind::Runbook,
            Self::Decision => RecordKind::Decision,
            Self::Gotcha => RecordKind::Gotcha,
            Self::Memory => RecordKind::Memory,
        }
    }

    /// Convert to the search-layer kind.
    pub(crate) fn to_index_kind(self) -> ft_search::IndexKind {
        ft_search::IndexKind::Record(self.to_core())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Output shape.
// ─────────────────────────────────────────────────────────────────────────────

/// Successful creation output — the freshly written record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedMemory {
    /// The new record (envelope + body).
    pub record: Record,
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-kind create inputs + ops.
// ─────────────────────────────────────────────────────────────────────────────

/// Input for [`create_incident`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIncidentInput {
    /// One-line summary.
    pub summary: String,
    /// Severity. Defaults to `sev3` when omitted.
    #[serde(default)]
    pub severity: Option<SeverityInput>,
    /// RFC3339 instant the incident began. Defaults to "now".
    #[serde(default)]
    #[cfg_attr(feature = "ts-rs", ts(type = "string | null"))]
    pub started_at: Option<DateTime<Utc>>,
    /// Comma-separated services affected (each entry trimmed).
    #[serde(default)]
    pub services: Vec<String>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Root-cause analysis, when one is known.
    #[serde(default)]
    pub root_cause: Option<String>,
    /// RFC3339 instant the incident was resolved, if known.
    #[serde(default)]
    #[cfg_attr(feature = "ts-rs", ts(type = "string | null"))]
    pub resolved_at: Option<DateTime<Utc>>,
    /// Record ids (full or prefix) of findings created from this incident.
    /// Each must resolve to a `Finding`.
    #[serde(default)]
    pub findings: Vec<String>,
    /// Record ids (full or prefix) of runbooks invoked while responding.
    /// Each must resolve to a `Runbook`.
    #[serde(default)]
    pub runbooks_invoked: Vec<String>,
}

/// `incident create` op.
pub fn create_incident(
    ws: &Workspace,
    identity: &Identity,
    input: CreateIncidentInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "incident create")?;
    let actor = ctx.actor.clone();

    // Resolve referenced findings / runbooks, validating each kind.
    let mut findings = Vec::with_capacity(input.findings.len());
    for raw in &input.findings {
        let id = ctx.resolve_id(raw)?;
        if id.kind() != RecordKind::Finding {
            return Err(OpsError::validation(
                "findings",
                format!("`{id}` is not a finding"),
            ));
        }
        findings.push(id);
    }
    let mut runbooks_invoked = Vec::with_capacity(input.runbooks_invoked.len());
    for raw in &input.runbooks_invoked {
        let id = ctx.resolve_id(raw)?;
        if id.kind() != RecordKind::Runbook {
            return Err(OpsError::validation(
                "runbooksInvoked",
                format!("`{id}` is not a runbook"),
            ));
        }
        runbooks_invoked.push(id);
    }

    let body = Incident {
        summary: input.summary.clone(),
        severity: input
            .severity
            .map_or(Severity::Sev3, SeverityInput::to_core),
        started_at: input.started_at.unwrap_or_else(Utc::now),
        resolved_at: input.resolved_at,
        services_affected: input.services,
        root_cause: input.root_cause,
        findings,
        runbooks_invoked,
        risk_class: input.risk_class.map(RiskClassInput::to_core),
        trust: TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Incident, &input.summary, actor)
        .incident(body)
        .origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

/// Input for [`create_finding`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFindingInput {
    /// One-line summary.
    pub summary: String,
    /// Parent incident id (full or prefix). Must resolve to an `Incident`.
    #[serde(default)]
    pub incident: Option<String>,
    /// Markdown details.
    #[serde(default)]
    pub details: Option<String>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Affected paths.
    #[serde(default)]
    pub affected: Vec<String>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// `finding create` op.
pub fn create_finding(
    ws: &Workspace,
    identity: &Identity,
    input: CreateFindingInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "finding create")?;
    let actor = ctx.actor.clone();

    let incident_id = if let Some(raw) = input.incident.as_deref() {
        let id = ctx.resolve_id(raw)?;
        if id.kind() != RecordKind::Incident {
            return Err(OpsError::validation(
                "incident",
                format!("`{id}` is not an incident"),
            ));
        }
        Some(id)
    } else {
        None
    };

    let body = Finding {
        summary: input.summary.clone(),
        details: input.details.unwrap_or_default(),
        incident: incident_id,
        risk_class: input.risk_class.map(RiskClassInput::to_core),
        affected_paths: input.affected,
        superseded_by: None,
        trust: TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Finding, &input.summary, actor)
        .finding(body)
        .origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

/// Input for [`create_runbook`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunbookInput {
    /// Short title.
    pub title: String,
    /// One-line summary describing when to use the runbook.
    pub summary: String,
    /// `applies_to` service names.
    #[serde(default)]
    pub applies_to: Vec<String>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// `runbook create` op.
pub fn create_runbook(
    ws: &Workspace,
    identity: &Identity,
    input: CreateRunbookInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "runbook create")?;
    let actor = ctx.actor.clone();
    let body = Runbook {
        title: input.title.clone(),
        summary: input.summary.clone(),
        steps: Vec::new(),
        applies_to: input.applies_to,
        risk_class: input.risk_class.map(RiskClassInput::to_core),
        trust: TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Runbook, &input.title, actor)
        .runbook(body)
        .origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

/// Input for [`create_decision`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionInput {
    /// Short title.
    pub title: String,
    /// Background / problem statement.
    pub context: String,
    /// The decision itself.
    pub decision: String,
    /// Consequences.
    #[serde(default)]
    pub consequences: Option<String>,
    /// Alternative options the team weighed.
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Content lifecycle status. Defaults to [`DecisionStatus::default`] when omitted.
    #[serde(default)]
    pub status: Option<DecisionStatusInput>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// `decision create` op.
pub fn create_decision(
    ws: &Workspace,
    identity: &Identity,
    input: CreateDecisionInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "decision create")?;
    let actor = ctx.actor.clone();
    let body = Decision {
        title: input.title.clone(),
        context: input.context,
        decision: input.decision,
        consequences: input.consequences.unwrap_or_default(),
        alternatives_considered: input.alternatives,
        status: input
            .status
            .map_or_else(DecisionStatus::default, DecisionStatusInput::to_core),
        superseded_by: None,
        risk_class: input.risk_class.map(RiskClassInput::to_core),
        trust: TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Decision, &input.title, actor)
        .decision(body)
        .origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

/// Input for [`create_gotcha`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGotchaInput {
    /// One-line summary.
    pub summary: String,
    /// Markdown details.
    #[serde(default)]
    pub details: Option<String>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Affected paths.
    #[serde(default)]
    pub affected: Vec<String>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// `gotcha create` op.
pub fn create_gotcha(
    ws: &Workspace,
    identity: &Identity,
    input: CreateGotchaInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "gotcha create")?;
    let actor = ctx.actor.clone();
    let body = Gotcha {
        summary: input.summary.clone(),
        details: input.details.unwrap_or_default(),
        affected_paths: input.affected,
        risk_class: input.risk_class.map(RiskClassInput::to_core),
        trust: TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Gotcha, &input.summary, actor)
        .gotcha(body)
        .origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

/// Input for [`create_memory`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemoryInput {
    /// Short title.
    pub title: String,
    /// Markdown body.
    pub body: String,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// `memory create` op (generic memory kind).
pub fn create_memory(
    ws: &Workspace,
    identity: &Identity,
    input: CreateMemoryInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "memory create")?;
    let actor = ctx.actor.clone();
    let body = Memory {
        title: input.title.clone(),
        body: input.body.clone(),
        tags: input.tags,
        related: Vec::new(),
        risk_class: input.risk_class.map(RiskClassInput::to_core),
        trust: TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Memory, &input.title, actor)
        .memory(body)
        .origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

// ─────────────────────────────────────────────────────────────────────────────
// capture — polymorphic over memory kind.
// ─────────────────────────────────────────────────────────────────────────────

/// Input for [`capture`].
///
/// The CLI's `firetrail capture` reads the body from `--body` or stdin; ops
/// require it as an explicit field. Adapters fill it in before calling.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInput {
    /// Memory kind. Defaults to [`MemoryKind::Memory`] when omitted.
    #[serde(default = "default_capture_kind")]
    pub kind: MemoryKind,
    /// Title / summary (required).
    pub title: String,
    /// Body text (required; must be non-empty after trimming).
    pub body: String,
    /// Tags (memory-kind only; ignored for other kinds).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Risk classification.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn default_capture_kind() -> MemoryKind {
    MemoryKind::Memory
}

/// `capture` op — write a memory-kind record from a body blob.
pub fn capture(
    ws: &Workspace,
    identity: &Identity,
    input: CaptureInput,
    events: &EventBus,
) -> Result<CreatedMemory, OpsError> {
    if input.body.trim().is_empty() {
        return Err(OpsError::validation("body", "body is empty"));
    }
    let mut ctx = MemoryCtx::open(ws, identity, "capture")?;
    let actor = ctx.actor.clone();
    let risk = input.risk_class.map(RiskClassInput::to_core);
    let body_text = input.body.clone();

    let mut builder =
        RecordBuilder::new(input.kind.to_core(), &input.title, actor).origin(Origin::Human);
    if let Some(s) = input.scope {
        builder = builder.owning_scope(s);
    }
    builder = match input.kind {
        MemoryKind::Memory => builder.memory(Memory {
            title: input.title.clone(),
            body: body_text.clone(),
            tags: input.tags.clone(),
            related: Vec::new(),
            risk_class: risk,
            trust: TrustState::Draft,
        }),
        MemoryKind::Gotcha => builder.gotcha(Gotcha {
            summary: input.title.clone(),
            details: body_text.clone(),
            affected_paths: Vec::new(),
            risk_class: risk,
            trust: TrustState::Draft,
        }),
        MemoryKind::Finding => builder.finding(Finding {
            summary: input.title.clone(),
            details: body_text.clone(),
            incident: None,
            risk_class: risk,
            affected_paths: Vec::new(),
            superseded_by: None,
            trust: TrustState::Draft,
        }),
        MemoryKind::Incident => builder.incident(Incident {
            summary: input.title.clone(),
            severity: Severity::Sev3,
            started_at: Utc::now(),
            resolved_at: None,
            services_affected: Vec::new(),
            root_cause: Some(body_text.clone()),
            findings: Vec::new(),
            runbooks_invoked: Vec::new(),
            risk_class: risk,
            trust: TrustState::Draft,
        }),
        MemoryKind::Runbook => builder.runbook(Runbook {
            title: input.title.clone(),
            summary: body_text.clone(),
            steps: Vec::new(),
            applies_to: Vec::new(),
            risk_class: risk,
            trust: TrustState::Draft,
        }),
        MemoryKind::Decision => builder.decision(Decision {
            title: input.title.clone(),
            context: String::new(),
            decision: body_text.clone(),
            consequences: String::new(),
            alternatives_considered: Vec::new(),
            status: DecisionStatus::default(),
            superseded_by: None,
            risk_class: risk,
            trust: TrustState::Draft,
        }),
    };

    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    ctx.save_record(&mut record)?;
    emit_created(events, input.request_id.as_deref(), &record);
    Ok(CreatedMemory { record })
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers.
// ─────────────────────────────────────────────────────────────────────────────

fn emit_created(bus: &EventBus, request_id: Option<&str>, record: &Record) {
    let record_kind = record.envelope.kind.prefix().to_ascii_lowercase();
    let event = Event::MemoryCreated {
        id: record.envelope.id.as_str().to_string(),
        record_kind,
    };
    if let Some(rid) = request_id {
        bus.emit_with_request(rid.to_string(), event);
    } else {
        bus.emit(event);
    }
}
