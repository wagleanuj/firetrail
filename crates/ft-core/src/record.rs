//! Record envelope and per-kind bodies.
//!
//! A [`Record`] is an [`RecordEnvelope`] plus a [`RecordBody`] variant. The
//! envelope carries the fields shared by every kind; the body carries the
//! kind-specific shape. Memory-kind bodies (Incident, Finding, Runbook,
//! Decision, Gotcha, Memory) declared empty at M1 carry real fields from M2.
//!
//! Each memory-kind body declares the trust/risk fields required by ADR-0013
//! (`trust: TrustState`, `risk_class: Option<RiskClass>`). `ft-core` only
//! declares and serializes these — the state-machine enforcement lives in
//! `ft-trust`.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::acceptance::{AcceptanceCriterion, Claim, Evidence};
use crate::enums::{Origin, Priority, RiskClass, Severity, Status, TrustState};
use crate::id::RecordId;
use crate::identity::Identity;
use crate::label::{HistoryEntry, Label};

/// Fields shared by every record kind.
///
/// Field order is fixed by the struct declaration; serde derives produce a
/// deterministic key order in `serde_json`'s output, which underpins the
/// canonical-JSON hashing performed by [`crate::hash::state_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordEnvelope {
    /// Canonical record identifier (ADR-0015).
    pub id: RecordId,
    /// Discriminator for the contained body.
    pub kind: crate::id::RecordKind,
    /// Short human-readable title.
    pub title: String,
    /// Workflow status.
    pub status: Status,
    /// Priority class.
    pub priority: Priority,
    /// Current owner of the record, if claimed.
    pub owner: Option<Identity>,
    /// Identity that created the record.
    pub created_by: Identity,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last-update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Close timestamp, if `status == Closed`.
    pub closed_at: Option<DateTime<Utc>>,

    /// Scope that owns the record (ADR-0004).
    pub owning_scope: Option<String>,
    /// Other scopes the record affects.
    pub affected_scopes: Vec<String>,
    /// Files / services the record applies to.
    pub applies_to: Vec<String>,

    /// Hash of canonical body+envelope, excluding the two hash fields
    /// themselves (ADR-0017).
    pub state_hash: String,
    /// `state_hash` of the prior version on the main branch (ADR-0017).
    /// Always `None` through M1; populated by `ft-history` from M2.
    pub prev_state_hash: Option<String>,

    /// Free-form labels.
    pub labels: Vec<Label>,

    /// Per-PR compacted history (ADR-0003). Always empty in M1; populated by
    /// `ft-history` from M2.
    pub history: Vec<HistoryEntry>,

    /// Provenance flag (ADR-0013).
    pub origin: Origin,
}

/// A long-lived effort that groups Tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Epic {
    /// Free-form description.
    pub description: String,
    /// Denormalized child record ids for fast reads.
    pub child_ids: Vec<RecordId>,
}

/// A unit of planned work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Task {
    /// Free-form description.
    pub description: String,
    /// Optional parent epic.
    pub parent_epic: Option<RecordId>,
    /// Acceptance criteria.
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    /// Attached evidence.
    pub evidence: Vec<Evidence>,
    /// Active claim, if any.
    pub claim: Option<Claim>,
}

/// A child of a Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Subtask {
    /// Free-form description.
    pub description: String,
    /// Required parent task.
    pub parent_task: RecordId,
    /// Acceptance criteria.
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    /// Attached evidence.
    pub evidence: Vec<Evidence>,
    /// Active claim, if any.
    pub claim: Option<Claim>,
}

/// A defect record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Bug {
    /// Free-form description.
    pub description: String,
    /// Affected service identifier.
    pub service: Option<String>,
    /// Severity classification (free-form string at M1).
    pub severity: Option<String>,
    /// Acceptance criteria for the fix.
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    /// Attached evidence.
    pub evidence: Vec<Evidence>,
    /// Active claim, if any.
    pub claim: Option<Claim>,
}

// ---------------------------------------------------------------------------
// Memory-kind bodies (writable from M2).
//
// Each carries the trust/risk fields required by ADR-0013. `ft-core` only
// declares and serializes them; the trust state machine lives in `ft-trust`.
// ---------------------------------------------------------------------------

/// Operational incident report.
///
/// Captures production reality at the moment the incident occurred. Lands via
/// a memory-only PR (ADR-0009) so the record outlives any single fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Incident {
    /// One-line summary of what happened.
    pub summary: String,
    /// Severity classification. Defaults to [`Severity::Sev3`].
    #[serde(default)]
    pub severity: Severity,
    /// Wall-clock time the incident began.
    pub started_at: DateTime<Utc>,
    /// Wall-clock time the incident was resolved, if known.
    pub resolved_at: Option<DateTime<Utc>>,
    /// Service / surface names impacted by the incident.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services_affected: Vec<String>,
    /// Root-cause analysis, when one is known.
    pub root_cause: Option<String>,
    /// Findings created from this incident.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<RecordId>,
    /// Runbooks invoked while responding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runbooks_invoked: Vec<RecordId>,
    /// Optional risk classification (ADR-0013).
    pub risk_class: Option<RiskClass>,
    /// Trust state. Declared here; state machine lives in `ft-trust`.
    #[serde(default = "default_trust")]
    pub trust: TrustState,
}

impl Default for Incident {
    fn default() -> Self {
        Self {
            summary: String::new(),
            severity: Severity::default(),
            started_at: epoch(),
            resolved_at: None,
            services_affected: Vec::new(),
            root_cause: None,
            findings: Vec::new(),
            runbooks_invoked: Vec::new(),
            risk_class: None,
            trust: TrustState::Draft,
        }
    }
}

/// Investigative finding — a discrete claim about how a system actually
/// behaves, captured for future readers and agents (ADR-0009).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Finding {
    /// One-line summary of the finding.
    pub summary: String,
    /// Long-form details (markdown body).
    #[serde(default)]
    pub details: String,
    /// Originating incident, if any.
    pub incident: Option<RecordId>,
    /// Optional risk classification (ADR-0013).
    pub risk_class: Option<RiskClass>,
    /// File paths the finding applies to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_paths: Vec<String>,
    /// Pointer to the record that replaced this one, if superseded.
    pub superseded_by: Option<RecordId>,
    /// Trust state. Declared here; state machine lives in `ft-trust`.
    #[serde(default = "default_trust")]
    pub trust: TrustState,
}

impl Default for Finding {
    fn default() -> Self {
        Self {
            summary: String::new(),
            details: String::new(),
            incident: None,
            risk_class: None,
            affected_paths: Vec::new(),
            superseded_by: None,
            trust: TrustState::Draft,
        }
    }
}

/// A single step in a [`Runbook`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct RunbookStep {
    /// Human-readable description of what the step accomplishes.
    pub description: String,
    /// Optional shell command (or other actionable invocation) for the step.
    pub command: Option<String>,
    /// What the operator should observe when the step succeeds.
    pub expected_outcome: String,
}

/// Operational runbook — an ordered list of steps an on-call engineer can
/// follow to handle a known situation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Runbook {
    /// Short title.
    pub title: String,
    /// One-line summary of when to use this runbook.
    pub summary: String,
    /// Ordered procedure steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<RunbookStep>,
    /// Service names / system tags this runbook applies to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applies_to: Vec<String>,
    /// Optional risk classification (ADR-0013).
    pub risk_class: Option<RiskClass>,
    /// Trust state. Declared here; state machine lives in `ft-trust`.
    #[serde(default = "default_trust")]
    pub trust: TrustState,
}

impl Default for Runbook {
    fn default() -> Self {
        Self {
            title: String::new(),
            summary: String::new(),
            steps: Vec::new(),
            applies_to: Vec::new(),
            risk_class: None,
            trust: TrustState::Draft,
        }
    }
}

/// Architectural / design decision record.
///
/// Captures context, the decision itself, consequences, and alternatives
/// considered — the canonical ADR shape adapted to Firetrail's record model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[allow(clippy::struct_field_names)]
pub struct Decision {
    /// Short title (e.g. "Use ONNX runtime for embeddings").
    pub title: String,
    /// Background / problem statement (markdown).
    #[serde(default)]
    pub context: String,
    /// The decision text itself.
    pub decision: String,
    /// Consequences of taking this decision.
    #[serde(default)]
    pub consequences: String,
    /// Alternative options the team weighed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives_considered: Vec<String>,
    /// Content lifecycle status of the decision itself.
    #[serde(default)]
    pub status: crate::enums::DecisionStatus,
    /// Pointer to the record that replaced this one, if superseded.
    pub superseded_by: Option<RecordId>,
    /// Optional risk classification (ADR-0013).
    pub risk_class: Option<RiskClass>,
    /// Trust state. Declared here; state machine lives in `ft-trust`.
    #[serde(default = "default_trust")]
    pub trust: TrustState,
}

impl Default for Decision {
    fn default() -> Self {
        Self {
            title: String::new(),
            context: String::new(),
            decision: String::new(),
            consequences: String::new(),
            alternatives_considered: Vec::new(),
            status: crate::enums::DecisionStatus::default(),
            superseded_by: None,
            risk_class: None,
            trust: TrustState::Draft,
        }
    }
}

/// A recurring footgun or sharp edge engineers keep encountering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Gotcha {
    /// One-line summary of the gotcha.
    pub summary: String,
    /// Long-form details (markdown).
    #[serde(default)]
    pub details: String,
    /// File paths the gotcha applies to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_paths: Vec<String>,
    /// Optional risk classification (ADR-0013).
    pub risk_class: Option<RiskClass>,
    /// Trust state. Declared here; state machine lives in `ft-trust`.
    #[serde(default = "default_trust")]
    pub trust: TrustState,
}

impl Default for Gotcha {
    fn default() -> Self {
        Self {
            summary: String::new(),
            details: String::new(),
            affected_paths: Vec::new(),
            risk_class: None,
            trust: TrustState::Draft,
        }
    }
}

/// Generic memory note — the catch-all body for memory-kind records that do
/// not fit into one of the more specific shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Memory {
    /// Short title.
    pub title: String,
    /// Markdown body.
    #[serde(default)]
    pub body: String,
    /// Free-form tags for indexing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Related records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RecordId>,
    /// Optional risk classification (ADR-0013).
    pub risk_class: Option<RiskClass>,
    /// Trust state. Declared here; state machine lives in `ft-trust`.
    #[serde(default = "default_trust")]
    pub trust: TrustState,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            title: String::new(),
            body: String::new(),
            tags: Vec::new(),
            related: Vec::new(),
            risk_class: None,
            trust: TrustState::Draft,
        }
    }
}

/// Default `trust` value for newly-deserialized memory bodies that omit the
/// field (forward-compat with pre-M2 records on disk).
fn default_trust() -> TrustState {
    TrustState::Draft
}

/// Earliest `DateTime<Utc>` representable: used as the `Default` value for
/// timestamp fields on memory bodies. Real records always set a real time.
fn epoch() -> DateTime<Utc> {
    chrono::DateTime::<Utc>::from_timestamp(0, 0).expect("unix epoch is representable")
}

/// Kind-specific body for a [`Record`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordBody {
    /// Epic body.
    Epic(Epic),
    /// Task body.
    Task(Task),
    /// Subtask body.
    Subtask(Subtask),
    /// Bug body.
    Bug(Bug),
    /// Incident body (memory kind, writable from M2).
    Incident(Incident),
    /// Finding body (memory kind, writable from M2).
    Finding(Finding),
    /// Runbook body (memory kind, writable from M2).
    Runbook(Runbook),
    /// Decision body (memory kind, writable from M2).
    Decision(Decision),
    /// Gotcha body (memory kind, writable from M2).
    Gotcha(Gotcha),
    /// Memory body (memory kind, writable from M2).
    Memory(Memory),
}

impl RecordBody {
    /// The `RecordKind` corresponding to this body variant.
    #[must_use]
    pub fn kind(&self) -> crate::id::RecordKind {
        use crate::id::RecordKind;
        match self {
            Self::Epic(_) => RecordKind::Epic,
            Self::Task(_) => RecordKind::Task,
            Self::Subtask(_) => RecordKind::Subtask,
            Self::Bug(_) => RecordKind::Bug,
            Self::Incident(_) => RecordKind::Incident,
            Self::Finding(_) => RecordKind::Finding,
            Self::Runbook(_) => RecordKind::Runbook,
            Self::Decision(_) => RecordKind::Decision,
            Self::Gotcha(_) => RecordKind::Gotcha,
            Self::Memory(_) => RecordKind::Memory,
        }
    }
}

/// A complete Firetrail record: envelope + body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Record {
    /// Common envelope fields.
    pub envelope: RecordEnvelope,
    /// Kind-specific body.
    pub body: RecordBody,
}
