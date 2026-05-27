//! [`TrustTransition`] — the audit-trail unit for the trust state machine.

use chrono::{DateTime, Utc};
use ft_core::{Evidence, Identity, Origin, RecordId, TrustState};
use serde::{Deserialize, Serialize};

/// A single request to move a record from one [`TrustState`] to another.
///
/// Per ADR-0013 the transition itself is the audit-trail unit: every legal
/// move produces one of these, and the caller (typically `ft-history`) appends
/// it to the record's compacted history alongside the new `state_hash`. The
/// state machine treats `TrustTransition` as a *value* — validation does not
/// mutate it.
///
/// `occurred_at` may be left unset (the default `Utc::now()` is provided by
/// [`TrustTransition::new`]). [`crate::apply_transition`] will populate it
/// from `Utc::now()` if the timestamp is still at the Unix epoch, so callers
/// can construct partially-populated transitions in tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustTransition {
    /// State the record was in before this transition.
    pub from: TrustState,
    /// State the record will be in after this transition.
    pub to: TrustState,
    /// Identity initiating the transition.
    pub reviewer: Identity,
    /// Provenance flag for the actor. ADR-0013 forbids `Origin::Agent` from
    /// promoting a record to [`TrustState::Verified`]. The state machine
    /// trusts the caller's classification — actual human-vs-agent attribution
    /// is ADR-0008 / `ft-identity` territory.
    pub origin: Origin,
    /// Free-form reason. Required for transitions into
    /// [`TrustState::Deprecated`], [`TrustState::Rejected`], and
    /// [`TrustState::Redacted`].
    pub reason: Option<String>,
    /// Evidence backing the transition. Required when promoting a
    /// high-stakes record (per [`ft_core::RiskClass::is_high_stakes`]) to
    /// [`TrustState::Verified`].
    pub evidence: Vec<Evidence>,
    /// Successor record. Required when transitioning to
    /// [`TrustState::Superseded`].
    pub successor: Option<RecordId>,
    /// When the transition happened. Set by [`crate::apply_transition`] if
    /// left at the Unix epoch.
    pub occurred_at: DateTime<Utc>,
}

impl TrustTransition {
    /// Construct a [`TrustTransition`] with `occurred_at` set to "now" by the
    /// caller. Convenience constructor; the struct's fields are all public
    /// and may be set directly.
    #[must_use]
    pub fn new(
        from: TrustState,
        to: TrustState,
        reviewer: Identity,
        origin: Origin,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            from,
            to,
            reviewer,
            origin,
            reason: None,
            evidence: Vec::new(),
            successor: None,
            occurred_at,
        }
    }
}
