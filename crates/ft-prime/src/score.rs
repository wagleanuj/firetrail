//! Deterministic priority scoring per ADR-0019.
//!
//! Score = `trust_weight * relevance * recency_factor`. The components are:
//!
//! - **Trust weight.** Verified=1.0, Reviewed=0.7, Draft=0.3,
//!   Stale/Deprecated=0.1, Archived/Rejected/Superseded/Redacted=0.0.
//! - **Recency factor.** Exponential decay with a 90-day half-life on
//!   `updated_at`.
//! - **Relevance.** Caller-supplied additive signal: 1.0 for a direct
//!   structural relation (parent epic, blocker, target itself), 0.5 for a
//!   query-term hit on title/body, 0.3 for a same-scope record. Multiple
//!   signals are summed and capped at 1.0.

use chrono::{DateTime, Utc};
use ft_core::TrustState;

/// Half-life used for the recency decay, in seconds (90 days).
const RECENCY_HALF_LIFE_SECS: f32 = 90.0 * 24.0 * 60.0 * 60.0;

/// Map a [`TrustState`] to a numeric weight.
#[must_use]
pub(crate) fn trust_weight(t: TrustState) -> f32 {
    match t {
        TrustState::Verified => 1.0,
        TrustState::Reviewed => 0.7,
        TrustState::Draft => 0.3,
        TrustState::Stale | TrustState::Deprecated => 0.1,
        TrustState::Archived
        | TrustState::Rejected
        | TrustState::Superseded
        | TrustState::Redacted => 0.0,
    }
}

/// Whether a record carrying `trust` clears the optional `floor`.
///
/// A record at or above the floor is kept; below it is dropped. The total
/// ordering used here matches the trust-weight ranks above.
#[must_use]
pub(crate) fn meets_trust_floor(trust: TrustState, floor: Option<TrustState>) -> bool {
    match floor {
        None => true,
        Some(f) => trust_weight(trust) >= trust_weight(f),
    }
}

/// Exponential-decay recency factor in `[0.0, 1.0]` based on `updated_at`.
///
/// `now` is the logical "current time" so callers (and tests) can pin output.
#[must_use]
pub(crate) fn recency_factor(updated_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let delta = (now - updated_at).num_seconds().max(0) as f32;
    let half_lives = delta / RECENCY_HALF_LIFE_SECS;
    // 2^(-half_lives) ; clamp lower bound so very old still has a small signal.
    (0.5_f32).powf(half_lives).clamp(0.001, 1.0)
}

/// Compose the final score. Components are clamped to `[0.0, 1.0]` after sum
/// so additive relevance signals cannot inflate above one.
#[must_use]
pub(crate) fn compose(trust: TrustState, relevance: f32, recency: f32) -> f32 {
    let r = relevance.clamp(0.0, 1.0);
    trust_weight(trust) * r * recency
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn trust_weights_order() {
        assert!(trust_weight(TrustState::Verified) > trust_weight(TrustState::Reviewed));
        assert!(trust_weight(TrustState::Reviewed) > trust_weight(TrustState::Draft));
        assert!(trust_weight(TrustState::Draft) > trust_weight(TrustState::Stale));
        assert!(trust_weight(TrustState::Archived) == 0.0);
    }

    #[test]
    fn recency_decays() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let fresh = recency_factor(now, now);
        let yesterday = recency_factor(now - chrono::Duration::days(1), now);
        let old = recency_factor(now - chrono::Duration::days(365), now);
        assert!((fresh - 1.0).abs() < 1e-6);
        assert!(yesterday < fresh);
        assert!(old < yesterday);
    }

    #[test]
    fn compose_prefers_verified() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let r = 1.0;
        let recency = recency_factor(now, now);
        let v = compose(TrustState::Verified, r, recency);
        let rv = compose(TrustState::Reviewed, r, recency);
        let d = compose(TrustState::Draft, r, recency);
        assert!(v > rv);
        assert!(rv > d);
    }

    #[test]
    fn meets_floor() {
        assert!(meets_trust_floor(
            TrustState::Verified,
            Some(TrustState::Reviewed)
        ));
        assert!(meets_trust_floor(
            TrustState::Reviewed,
            Some(TrustState::Reviewed)
        ));
        assert!(!meets_trust_floor(
            TrustState::Draft,
            Some(TrustState::Reviewed)
        ));
        assert!(meets_trust_floor(TrustState::Draft, None));
    }
}
