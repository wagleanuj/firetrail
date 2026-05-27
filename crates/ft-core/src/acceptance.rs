//! Acceptance criteria, evidence, and claim types.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::enums::{AcStatus, EvidenceKind};
use crate::identity::Identity;

/// An acceptance criterion attached to a Task / Subtask / Bug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AcceptanceCriterion {
    /// Local stable id within the parent record (`ac-01`, `ac-02`, …).
    pub id: String,
    /// Human-readable criterion text.
    pub text: String,
    /// Whether the criterion is currently satisfied.
    pub status: AcStatus,
    /// URL pointing to evidence backing the `Checked` state.
    pub evidence_url: Option<String>,
    /// Identity that flipped `status` to `Checked`.
    pub checked_by: Option<Identity>,
    /// Timestamp the criterion was marked `Checked`.
    pub checked_at: Option<DateTime<Utc>>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last-update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Set to `true` for agent-proposed criteria pending human confirmation
    /// (ADR-0013).
    pub proposed: bool,
}

/// A piece of evidence attached to a record (acceptance criterion or finding).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    /// Local id within the parent record (`ev-01`, …).
    pub id: String,
    /// Artefact classification.
    pub kind: EvidenceKind,
    /// Canonical URL of the evidence.
    pub url: String,
    /// Optional free-form description.
    pub description: Option<String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Identity that attached this evidence.
    pub created_by: Identity,
    /// Optional commit SHA the evidence was captured at.
    pub commit_sha: Option<String>,
    /// Optional code symbol the evidence references.
    pub symbol_name: Option<String>,
    /// Optional content hash of the linked artefact.
    pub content_hash: Option<String>,
}

/// A claim of ownership over a record (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[allow(clippy::struct_field_names)]
pub struct Claim {
    /// Identity that currently owns the work.
    pub claimed_by: Identity,
    /// When the claim was filed.
    pub claimed_at: DateTime<Utc>,
    /// Free-form provenance (`cli`, `agent:claude-code`, …).
    pub claim_source: String,
    /// When the claim auto-releases (mandatory per ADR-0008).
    pub claim_expires_at: DateTime<Utc>,
}
