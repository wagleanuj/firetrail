//! Enumerated fields shared across records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Workflow status for a work-tracking record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Created but not triaged.
    Open,
    /// Triaged; available to be claimed.
    Ready,
    /// Actively being worked.
    InProgress,
    /// Awaiting review.
    Review,
    /// Blocked on something external.
    Blocked,
    /// Done and accepted.
    Closed,
    /// Postponed without commitment.
    Deferred,
    /// Closed for historical purposes only.
    Archived,
}

/// Priority class. `P0` is critical; `P4` is backlog. Mirrors `bd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Critical, top-of-queue.
    P0,
    /// High priority.
    P1,
    /// Normal priority.
    P2,
    /// Low priority.
    P3,
    /// Backlog.
    P4,
}

/// Provenance of a record (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Created by a human author.
    Human,
    /// Created by an agent on behalf of a human.
    Agent,
    /// Imported from an external system.
    Imported,
}

/// Status of a single acceptance criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcStatus {
    /// Not yet satisfied.
    Unchecked,
    /// Satisfied, with evidence attached.
    Checked,
}

/// What kind of artefact a piece of evidence references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// External incident report.
    IncidentReport,
    /// A pull request URL.
    PullRequest,
    /// A specific commit.
    Commit,
    /// Operational dashboard.
    Dashboard,
    /// Saved log query.
    LogQuery,
    /// Test run output.
    TestResult,
    /// A Jira ticket.
    JiraTicket,
    /// A Confluence page.
    ConfluencePage,
    /// Free-form note added manually.
    ManualNote,
}

/// Relationship classes between records (FR-020).
///
/// The M1 writable subset is `Blocks`, `BlockedBy`, `ParentOf`, `ChildOf`,
/// `RelatedTo`, `Duplicates`, `Supersedes`. Remaining variants are declared
/// for forward compatibility and round-trip cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    /// `from` blocks `to`.
    Blocks,
    /// `from` is blocked by `to`.
    BlockedBy,
    /// `from` is the parent of `to`.
    ParentOf,
    /// `from` is a child of `to`.
    ChildOf,
    /// `from` is related to `to` (weak link).
    RelatedTo,
    /// `from` duplicates `to`.
    Duplicates,
    /// `from` supersedes `to`.
    Supersedes,
    /// `from` was discovered while working on `to`.
    DiscoveredDuring,
    /// `from` is a follow-up to `to`.
    FollowUpFrom,
    /// `from` is fixed by `to`.
    FixedBy,
    /// `from` was caused by `to`.
    CausedBy,
    /// `from` is mitigated by `to`.
    MitigatedBy,
    /// `from` is documented in `to`.
    DocumentedIn,
    /// `from` is implemented by `to`.
    ImplementedBy,
    /// `from` was regressed by `to`.
    RegressedBy,
    /// `from` affects `to`.
    Affects,
    /// `from` is owned by `to`.
    OwnedBy,
}

impl RelationKind {
    /// Whether this relation kind is part of the M1 writable subset.
    #[must_use]
    pub fn writable_in_m1(self) -> bool {
        matches!(
            self,
            Self::Blocks
                | Self::BlockedBy
                | Self::ParentOf
                | Self::ChildOf
                | Self::RelatedTo
                | Self::Duplicates
                | Self::Supersedes
        )
    }
}

/// Trust state for memory-kind records (declared at M1; enforced in M2 by
/// `ft-trust`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    /// Newly authored, unreviewed.
    Draft,
    /// Reviewed by a human.
    Reviewed,
    /// Independently verified.
    Verified,
    /// Content has aged past its freshness window.
    Stale,
    /// Marked as no longer best-practice.
    Deprecated,
    /// Closed for historical reference only.
    Archived,
    /// Replaced by another record.
    Superseded,
    /// Rejected during review.
    Rejected,
    /// Content removed for compliance.
    Redacted,
}

/// Risk classification of a memory-kind record (declared at M1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Security risk.
    Security,
    /// Availability risk.
    Availability,
    /// Data-loss risk.
    DataLoss,
    /// Regulatory / compliance risk.
    Compliance,
    /// Performance risk.
    Performance,
    /// Correctness risk.
    Correctness,
}
