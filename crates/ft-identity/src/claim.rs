//! Claim lifecycle helpers (ADR-0008, M5).
//!
//! The actual claim records (the `claim` field on a [`ft_core::Task`] body)
//! are persisted by `ft-storage` and mutated by `ft-cli`. The helpers here
//! are the pure-logic pieces that policy code (in this crate and in `ft-cli`)
//! needs to reason about claims: expiry, takeover eligibility, and the
//! `on-behalf-of` relation between a CI actor and a human author.

use chrono::{DateTime, Utc};
use ft_core::Identity;

use crate::registry::IdentityRegistry;

/// Information about a live claim, as seen by the policy layer.
///
/// This is the M5 logical view; the wire representation lives on the task
/// body in `ft-core` as [`ft_core::Claim`]. The `ft-cli` claim subcommands
/// build a [`ClaimInfo`] from the underlying record before consulting these
/// helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimInfo {
    /// The identity that holds the claim.
    pub actor: Identity,
    /// When the claim auto-releases.
    pub claim_expires_at: DateTime<Utc>,
    /// If the actor is a bot or CI runner acting for a human, the human's
    /// identity (resolved via `parse_on_behalf_of` or a similar trailer).
    pub on_behalf_of: Option<Identity>,
}

/// Is the claim past its expiry instant relative to `now`?
///
/// Equality (`now == claim_expires_at`) counts as expired, matching git's
/// usual "at or after" semantics for time gates.
#[must_use]
pub fn is_claim_expired(claim: &ClaimInfo, now: DateTime<Utc>) -> bool {
    now >= claim.claim_expires_at
}

/// May `taker` take over `claim`?
///
/// True when either:
///
/// - the claim is expired (anyone may pick it up); or
/// - the taker has the `can_force_push` capability in the registry. That
///   capability is intentionally the admin-only bit — only admins can yank
///   live claims off someone else.
///
/// Bots and CI runners with admin overrides still pass; the check is
/// capability-driven, not kind-driven.
#[must_use]
pub fn can_take_over(
    claim: &ClaimInfo,
    taker: &Identity,
    registry: &IdentityRegistry,
    now: DateTime<Utc>,
) -> bool {
    if is_claim_expired(claim, now) {
        return true;
    }
    // Match the taker against the registry by id or any alias.
    let Some(reg) = registry.resolve_canonical(taker.as_str()) else {
        return false;
    };
    registry.can(&reg.id, "can_force_push")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{IdentityKind, IdentityStatus};
    use crate::registry::{PartialCapabilityMatrix, RegisteredIdentity};

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn alice_claim() -> ClaimInfo {
        ClaimInfo {
            actor: Identity::new("alice@example.com").unwrap(),
            claim_expires_at: at("2026-06-01T00:00:00Z"),
            on_behalf_of: None,
        }
    }

    fn registry_with_admin(admin_id: &str) -> IdentityRegistry {
        IdentityRegistry {
            identities: vec![
                RegisteredIdentity {
                    id: "alice".into(),
                    name: "Alice".into(),
                    kind: IdentityKind::Human,
                    emails: vec!["alice@example.com".into()],
                    machines: vec![],
                    capabilities: PartialCapabilityMatrix::default(),
                    status: IdentityStatus::Active,
                },
                RegisteredIdentity {
                    id: admin_id.into(),
                    name: "Admin".into(),
                    kind: IdentityKind::Human,
                    emails: vec!["admin@example.com".into()],
                    machines: vec![],
                    capabilities: PartialCapabilityMatrix {
                        can_force_push: Some(true),
                        ..Default::default()
                    },
                    status: IdentityStatus::Active,
                },
            ],
        }
    }

    #[test]
    fn expired_at_or_after_expiry_instant() {
        let claim = alice_claim();
        assert!(is_claim_expired(&claim, at("2026-06-01T00:00:00Z")));
        assert!(is_claim_expired(&claim, at("2026-06-02T00:00:00Z")));
    }

    #[test]
    fn not_expired_before_expiry() {
        let claim = alice_claim();
        assert!(!is_claim_expired(&claim, at("2026-05-31T23:59:59Z")));
    }

    #[test]
    fn takeover_allowed_when_expired() {
        let claim = alice_claim();
        let registry = registry_with_admin("bob");
        let taker = Identity::new("bob@example.com").unwrap();
        // Bob has no admin cap but the claim is expired.
        let registry_no_admin = IdentityRegistry::empty();
        assert!(can_take_over(
            &claim,
            &taker,
            &registry_no_admin,
            at("2026-07-01T00:00:00Z"),
        ));
        let _ = registry; // silence unused
    }

    #[test]
    fn takeover_allowed_when_taker_has_admin() {
        let claim = alice_claim();
        let registry = registry_with_admin("admin");
        let taker = Identity::new("admin@example.com").unwrap();
        assert!(can_take_over(
            &claim,
            &taker,
            &registry,
            at("2026-05-15T00:00:00Z"),
        ));
    }

    #[test]
    fn takeover_denied_when_claim_live_and_taker_lacks_admin() {
        let claim = alice_claim();
        let registry = registry_with_admin("admin");
        let taker = Identity::new("alice@example.com").unwrap();
        assert!(!can_take_over(
            &claim,
            &taker,
            &registry,
            at("2026-05-15T00:00:00Z"),
        ));
    }

    #[test]
    fn takeover_denied_when_taker_unknown_to_registry() {
        let claim = alice_claim();
        let registry = registry_with_admin("admin");
        let taker = Identity::new("stranger@example.com").unwrap();
        assert!(!can_take_over(
            &claim,
            &taker,
            &registry,
            at("2026-05-15T00:00:00Z"),
        ));
    }
}
