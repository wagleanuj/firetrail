//! Trust state machine: validation and application of [`TrustTransition`]s.
//!
//! The state machine encodes the rules from ADR-0013:
//!
//! - `Draft → Reviewed` — one reviewer, distinct from the record author.
//! - `Reviewed → Verified` — a second, distinct reviewer; *must* be a human
//!   ([`Origin::Agent`] is rejected with [`TrustError::AgentCannotPromote`]).
//!   High-stakes records (see [`ft_core::RiskClass::is_high_stakes`]) require
//!   at least one [`Evidence`] entry.
//! - `* → Stale` — typically computed by [`crate::is_stale`], but accepted
//!   through this API so callers can route Stale through the same audit pipe.
//! - `* → Deprecated` — manual; reason required.
//! - `* → Archived` — terminal; closes the record's lifecycle.
//! - `* → Superseded` — terminal; requires a successor record id.
//! - `Draft|Reviewed → Rejected` — terminal; reason required.
//! - `* → Redacted` — terminal; reason required. Body content is wiped by
//!   [`crate::apply_transition`] per ADR-0013.
//!
//! Terminal states are [`TrustState::Archived`], [`TrustState::Superseded`],
//! [`TrustState::Rejected`], and [`TrustState::Redacted`]. No transition out
//! of a terminal state is legal.

use chrono::{DateTime, Utc};
use ft_core::{Identity, Origin, RiskClass, TrustState};

use crate::body::MemoryBody;
use crate::error::TrustError;
use crate::transition::TrustTransition;

/// Number of distinct reviewers required to reach [`TrustState::Verified`]
/// per ADR-0013.
pub const VERIFIED_REVIEWER_COUNT: usize = 2;

/// Returns `true` if `state` is a terminal trust state (no legal exit).
#[must_use]
pub fn is_terminal(state: TrustState) -> bool {
    matches!(
        state,
        TrustState::Archived | TrustState::Superseded | TrustState::Rejected | TrustState::Redacted
    )
}

/// Validate that `requested` is a legal transition for a record currently in
/// `current_state`, authored by `record_author`, of risk class `risk_class`,
/// with `prior_reviewers` having already promoted it on earlier edges.
///
/// Returns `Ok(())` if the transition may be applied. See [`TrustError`] for
/// the failure modes.
///
/// # Errors
///
/// See [`TrustError`] for the full taxonomy.
pub fn validate_transition(
    current_state: TrustState,
    risk_class: Option<RiskClass>,
    requested: &TrustTransition,
    prior_reviewers: &[Identity],
    record_author: &Identity,
) -> Result<(), TrustError> {
    // The transition's `from` must match the record's actual state.
    if requested.from != current_state {
        return Err(TrustError::IllegalTransition {
            from: requested.from,
            to: requested.to,
        });
    }

    // No legal exits from a terminal state.
    if is_terminal(current_state) {
        return Err(TrustError::IllegalTransition {
            from: current_state,
            to: requested.to,
        });
    }

    // Identical from→to is not a transition.
    if requested.from == requested.to {
        return Err(TrustError::IllegalTransition {
            from: requested.from,
            to: requested.to,
        });
    }

    // Edge legality.
    #[allow(clippy::match_same_arms)]
    match (current_state, requested.to) {
        // Promotion ladder.
        (TrustState::Draft, TrustState::Reviewed) => {
            check_distinct_reviewer(requested, record_author, prior_reviewers)?;
            Ok(())
        }
        (TrustState::Reviewed, TrustState::Verified) => {
            // Agents may never promote to Verified.
            if requested.origin == Origin::Agent {
                return Err(TrustError::AgentCannotPromote { to: requested.to });
            }
            check_distinct_reviewer(requested, record_author, prior_reviewers)?;
            // Need at least VERIFIED_REVIEWER_COUNT distinct reviewers total
            // (counting the current request).
            let mut seen: Vec<&Identity> = Vec::with_capacity(prior_reviewers.len() + 1);
            for r in prior_reviewers {
                if !seen.contains(&r) {
                    seen.push(r);
                }
            }
            if !seen.contains(&&requested.reviewer) {
                seen.push(&requested.reviewer);
            }
            if seen.len() < VERIFIED_REVIEWER_COUNT {
                return Err(TrustError::InsufficientReviewers {
                    required: VERIFIED_REVIEWER_COUNT,
                    present: seen.len(),
                });
            }
            // High-stakes records require evidence to reach Verified.
            if risk_class.is_some_and(RiskClass::is_high_stakes) && requested.evidence.is_empty() {
                return Err(TrustError::EvidenceRequired { kind: requested.to });
            }
            Ok(())
        }

        // Rejected only from Draft or Reviewed; reason required. (Rejected
        // from any other state falls through to the wildcard arm and is
        // rejected as an illegal edge.)
        (TrustState::Draft | TrustState::Reviewed, TrustState::Rejected) => {
            require_reason(requested)?;
            Ok(())
        }

        // Stale — computed transition; accepted from any non-terminal state.
        // Archived — terminal; from any non-terminal state, no extras.
        (_, TrustState::Stale | TrustState::Archived) => Ok(()),

        // Deprecated — manual; reason required; from any non-terminal state.
        (_, TrustState::Deprecated) => {
            require_reason(requested)?;
            Ok(())
        }

        // Superseded — terminal; requires successor.
        (_, TrustState::Superseded) => {
            if requested.successor.is_none() {
                return Err(TrustError::MissingSuccessor);
            }
            Ok(())
        }

        // Redacted — terminal; reason required.
        (_, TrustState::Redacted) => {
            require_reason(requested)?;
            Ok(())
        }

        // Everything else is an illegal edge (e.g. Draft → Verified directly).
        _ => Err(TrustError::IllegalTransition {
            from: current_state,
            to: requested.to,
        }),
    }
}

/// Check the reviewer is neither the author nor a prior promoter.
fn check_distinct_reviewer(
    requested: &TrustTransition,
    record_author: &Identity,
    prior_reviewers: &[Identity],
) -> Result<(), TrustError> {
    if &requested.reviewer == record_author {
        return Err(TrustError::SelfReview);
    }
    if prior_reviewers.iter().any(|r| r == &requested.reviewer) {
        return Err(TrustError::DuplicateReviewer {
            reviewer: requested.reviewer.clone(),
        });
    }
    Ok(())
}

fn require_reason(requested: &TrustTransition) -> Result<(), TrustError> {
    if requested
        .reason
        .as_deref()
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err(TrustError::MissingReason { kind: requested.to });
    }
    Ok(())
}

/// Apply `transition` to `body`, mutating its `trust` field in place.
///
/// The function does *not* re-validate — call [`validate_transition`] first.
/// It does:
///
/// 1. Set `body.trust = transition.to`.
/// 2. If `transition.to == Redacted`, wipe body content per ADR-0013.
/// 3. If `transition.occurred_at` is at the Unix epoch, replace it with
///    `Utc::now()` so the returned transition carries a real timestamp.
///
/// Returns the (possibly timestamp-fixed) transition for the caller to append
/// to the record's history. Updating `state_hash` / `prev_state_hash` is the
/// job of `ft-history`.
///
/// # Errors
///
/// Currently infallible at the apply step (validation lives in
/// [`validate_transition`]). Returns `Result` so future tightening (e.g. body
/// invariants that depend on the target state) can be added without a
/// breaking API change.
#[allow(clippy::unnecessary_wraps)]
pub fn apply_transition(
    body: &mut MemoryBody<'_>,
    transition: &TrustTransition,
) -> Result<TrustTransition, TrustError> {
    let mut applied = transition.clone();
    if applied.occurred_at == epoch() {
        applied.occurred_at = Utc::now();
    }

    set_trust(body, applied.to);

    if applied.to == TrustState::Redacted {
        redact_body(body);
    }

    Ok(applied)
}

/// Overwrite the `trust` field of a [`MemoryBody`] uniformly across variants.
fn set_trust(body: &mut MemoryBody<'_>, to: TrustState) {
    match body {
        MemoryBody::Incident(b) => b.trust = to,
        MemoryBody::Finding(b) => b.trust = to,
        MemoryBody::Runbook(b) => b.trust = to,
        MemoryBody::Decision(b) => b.trust = to,
        MemoryBody::Gotcha(b) => b.trust = to,
        MemoryBody::Memory(b) => b.trust = to,
    }
}

/// Wipe content fields per ADR-0013's redaction rules. Metadata (timestamps,
/// risk class, trust, identifiers) is preserved.
fn redact_body(body: &mut MemoryBody<'_>) {
    match body {
        MemoryBody::Incident(b) => {
            b.summary.clear();
            b.root_cause = None;
            b.services_affected.clear();
        }
        MemoryBody::Finding(b) => {
            b.summary.clear();
            b.details.clear();
            b.affected_paths.clear();
        }
        MemoryBody::Runbook(b) => {
            b.title.clear();
            b.summary.clear();
            b.steps.clear();
            b.applies_to.clear();
        }
        MemoryBody::Decision(b) => {
            b.title.clear();
            b.context.clear();
            b.decision.clear();
            b.consequences.clear();
            b.alternatives_considered.clear();
        }
        MemoryBody::Gotcha(b) => {
            b.summary.clear();
            b.details.clear();
            b.affected_paths.clear();
        }
        MemoryBody::Memory(b) => {
            b.title.clear();
            b.body.clear();
            b.tags.clear();
            b.related.clear();
        }
    }
}

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).expect("unix epoch is representable")
}
