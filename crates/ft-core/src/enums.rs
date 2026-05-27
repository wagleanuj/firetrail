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

/// Risk classification of a memory-kind record (ADR-0013).
///
/// The first four variants (`Security`, `Availability`, `DataLoss`,
/// `Compliance`) are *high-stakes* — records carrying them require `verified`
/// trust before appearing in default `prime` output and need 180-day
/// re-validation. The remaining variants (`Performance`, `Correctness`) are
/// low-stakes and follow the standard trust rules.
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

impl RiskClass {
    /// Whether this risk class is high-stakes per ADR-0013.
    ///
    /// High-stakes records (security, availability, data-loss, compliance)
    /// require `verified` trust before default prime inclusion. The
    /// state-machine enforcement of this rule lives in `ft-trust`.
    #[must_use]
    pub fn is_high_stakes(self) -> bool {
        matches!(
            self,
            Self::Security | Self::Availability | Self::DataLoss | Self::Compliance
        )
    }
}

/// Severity classification for an [`crate::record::Incident`].
///
/// `Sev1` is the most severe (customer-impacting outage class), `Sev4` is the
/// least severe (minor / informational). Mirrors common SRE conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Critical, customer-impacting outage.
    Sev1,
    /// Major degradation; some users affected.
    Sev2,
    /// Minor impact; partial degradation.
    #[default]
    Sev3,
    /// Informational; no user-visible impact.
    Sev4,
}

/// Lifecycle status of an architectural decision record.
///
/// Distinct from the broader [`TrustState`] in that it describes the *content
/// posture* of a decision (still being proposed, accepted, replaced) rather
/// than its review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    /// Drafted and under discussion.
    #[default]
    Proposed,
    /// Accepted and current.
    Accepted,
    /// Replaced by a successor decision (see `superseded_by`).
    Superseded,
    /// No longer applicable but kept for audit.
    Deprecated,
}
