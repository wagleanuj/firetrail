//! Record envelope and per-kind bodies.
//!
//! A [`Record`] is an [`RecordEnvelope`] plus a [`RecordBody`] variant. The
//! envelope carries the fields shared by every kind; the body carries the
//! kind-specific shape. Memory-kind bodies (Incident, Finding, Runbook,
//! Decision, Gotcha, Memory) are declared at M1 as empty placeholders so the
//! on-disk schema version is locked; they become writable from M2.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::acceptance::{AcceptanceCriterion, Claim, Evidence};
use crate::enums::{Origin, Priority, Status};
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

/// Memory-kind body placeholders.
///
/// These types round-trip through serde at M1 to lock the on-disk schema
/// version, but the [`crate::builder::RecordBuilder`] refuses to construct
/// records of these kinds until M2 wires the corresponding feature work.
macro_rules! memory_body {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema,
        )]
        pub struct $name {
            /// Reserved for future fields. Round-trips as `{}` at M1.
            #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
            pub reserved: serde_json::Map<String, serde_json::Value>,
        }
    };
}

memory_body!(
    /// Incident record (memory kind, writable from M2).
    Incident
);
memory_body!(
    /// Investigative finding (memory kind, writable from M2).
    Finding
);
memory_body!(
    /// Operational runbook (memory kind, writable from M2).
    Runbook
);
memory_body!(
    /// Architectural decision record (memory kind, writable from M2).
    Decision
);
memory_body!(
    /// Recurring footgun (memory kind, writable from M2).
    Gotcha
);
memory_body!(
    /// Generic memory note (memory kind, writable from M2).
    Memory
);

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
