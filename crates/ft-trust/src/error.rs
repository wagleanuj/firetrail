//! Errors raised by the trust state machine (ADR-0013).

use ft_core::{Identity, TrustState};
use thiserror::Error;

/// Errors produced by [`crate::validate_transition`] and
/// [`crate::apply_transition`].
///
/// Each variant maps to a specific clause of ADR-0013. The error carries
/// enough context for the CLI to render an actionable message without having
/// to reconstruct the request.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub enum TrustError {
    /// The `(from, to)` pair is not a legal edge in the trust state machine.
    #[error("illegal trust transition from {from:?} to {to:?}")]
    IllegalTransition {
        /// State the record is currently in.
        from: TrustState,
        /// State the caller requested.
        to: TrustState,
    },

    /// Agents (`Origin::Agent`) cannot promote a record to
    /// [`TrustState::Verified`]. Per ADR-0013, verification requires a human.
    #[error("agents cannot promote a record to {to:?}; a human reviewer is required")]
    AgentCannotPromote {
        /// State the agent attempted to promote to (always
        /// [`TrustState::Verified`] in practice).
        to: TrustState,
    },

    /// The reviewer attempting the transition is the record author. ADR-0013
    /// requires reviewer ≠ author for the first promotion edge.
    #[error("a reviewer cannot review their own record")]
    SelfReview,

    /// The reviewer has already promoted this record once on a prior edge.
    /// ADR-0013 requires distinct reviewers for each promotion step so that
    /// `Verified` always reflects two independent humans.
    #[error("reviewer {reviewer} has already promoted this record")]
    DuplicateReviewer {
        /// Identity that has already participated in promoting this record.
        reviewer: Identity,
    },

    /// Reaching the requested state requires more distinct reviewers than the
    /// record currently has.
    #[error("transition requires {required} reviewers; only {present} present")]
    InsufficientReviewers {
        /// How many distinct reviewers ADR-0013 requires for this edge.
        required: usize,
        /// How many distinct reviewers the record currently has (including
        /// the one in the current request).
        present: usize,
    },

    /// A transition that requires a free-form reason was missing one.
    /// Applies to [`TrustState::Deprecated`], [`TrustState::Rejected`], and
    /// [`TrustState::Redacted`].
    #[error("transition to {kind:?} requires a non-empty reason")]
    MissingReason {
        /// The target state the request was for.
        kind: TrustState,
    },

    /// A [`TrustState::Superseded`] transition was missing its `successor`
    /// pointer.
    #[error("transition to Superseded requires a successor record id")]
    MissingSuccessor,

    /// A promotion to [`TrustState::Verified`] for a high-stakes record was
    /// missing the evidence ADR-0013 requires (a linked test, postmortem, or
    /// production validation).
    #[error("transition to {kind:?} for a high-stakes record requires at least one evidence entry")]
    EvidenceRequired {
        /// The target state the request was for (always
        /// [`TrustState::Verified`] in practice).
        kind: TrustState,
    },
}
