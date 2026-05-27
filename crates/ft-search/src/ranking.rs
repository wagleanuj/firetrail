//! Ranking weights and helpers.
//!
//! ## Weight choice
//!
//! Defaults are chosen so a strong vector match (semantic recall) dominates
//! when both signals fire, with FTS contributing meaningful evidence for
//! keyword-exact queries. Trust contributes a non-trivial multiplier so a
//! `Verified` runbook beats a `Draft` memory of equal similarity, and recency
//! is a small tie-breaker — old verified knowledge should still rank above
//! new noise.
//!
//! `α + β + γ + δ = 1.0`.

use chrono::{DateTime, Utc};
use ft_core::TrustState;

/// Vector-similarity weight. See module docs.
pub const ALPHA: f32 = 0.50;
/// Lexical (FTS5 bm25-derived) weight.
pub const BETA: f32 = 0.30;
/// Trust-weight multiplier in the additive sum (separate from the
/// post-multiplication done by [`trust_weight`]).
pub const GAMMA: f32 = 0.15;
/// Recency weight.
pub const DELTA: f32 = 0.05;

/// Half-life (in days) used by [`recency_weight`].
///
/// A 90-day half-life means a record updated a quarter ago contributes ~0.5,
/// a record updated a year ago contributes ~0.06.
pub const RECENCY_HALF_LIFE_DAYS: f64 = 90.0;

/// Map a [`TrustState`] to a multiplier in `[0.0, 1.0]`.
///
/// Verified records are surfaced at full weight. Draft records are heavily
/// discounted so they rank below reviewed content with comparable similarity.
/// Terminal states (`Archived`, `Superseded`, `Rejected`, `Redacted`) are
/// scored at zero so they cannot win a ranking on their own; callers that
/// want to surface them must request them explicitly via `kind_filter` and a
/// disabled `min_trust`.
#[must_use]
pub fn trust_weight(trust: TrustState) -> f32 {
    match trust {
        TrustState::Verified => 1.0,
        TrustState::Reviewed => 0.7,
        TrustState::Draft => 0.3,
        TrustState::Stale | TrustState::Deprecated => 0.1,
        TrustState::Archived
        | TrustState::Superseded
        | TrustState::Rejected
        | TrustState::Redacted => 0.0,
    }
}

/// Total ordering on [`TrustState`] used to enforce `min_trust` filters.
///
/// Higher = more trustworthy. Mirrors [`trust_weight`] tiers.
#[must_use]
pub fn trust_rank(trust: TrustState) -> u8 {
    match trust {
        TrustState::Verified => 6,
        TrustState::Reviewed => 5,
        TrustState::Draft => 4,
        TrustState::Stale => 3,
        TrustState::Deprecated => 2,
        TrustState::Archived | TrustState::Superseded => 1,
        TrustState::Rejected | TrustState::Redacted => 0,
    }
}

/// Convert a BM25 score (lower = better; 0 = perfect match in FTS5's signed
/// convention) to a [0, 1] weight (higher = better).
///
/// FTS5 returns negative bm25 values by default; we invert and squash with
/// `1 / (1 + |b|)` which keeps the mapping monotonic and bounded.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn normalize_bm25(raw: f64) -> f32 {
    let abs = raw.abs();
    let normalized = 1.0 / (1.0 + abs);
    // Clamp defensively in case of pathological inputs (NaN/inf).
    if normalized.is_finite() {
        normalized.clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Compute the recency weight for an `updated_at` timestamp, relative to
/// `now`. Result is in `[0, 1]`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn recency_weight(updated_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    let secs = (now - updated_at).num_seconds().max(0);
    // num_seconds returns i64; cast through f64 with documented precision loss.
    let age_days = secs as f64 / 86_400.0;
    let decay = (-age_days * std::f64::consts::LN_2 / RECENCY_HALF_LIFE_DAYS).exp();
    if decay.is_finite() {
        decay.clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Final score combiner used in `SearchMode::Hybrid`.
#[must_use]
pub fn hybrid_score(
    vector_sim: f32,
    lexical_score: f32,
    trust: TrustState,
    updated_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f32 {
    let tw = trust_weight(trust);
    let rw = recency_weight(updated_at, now);
    let combined = ALPHA * vector_sim + BETA * lexical_score + GAMMA * tw + DELTA * rw;
    // Multiply by trust so terminal-state records can never win.
    combined * tw
}

/// Score combiner for lexical-only hits.
#[must_use]
pub fn lexical_only_score(
    lexical_score: f32,
    trust: TrustState,
    updated_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f32 {
    let tw = trust_weight(trust);
    let rw = recency_weight(updated_at, now);
    // Lexical weight dominates, but keep trust + recency contributions.
    let combined = lexical_score + GAMMA * tw + DELTA * rw;
    combined * tw
}

/// Score combiner for vector-only hits.
#[must_use]
pub fn vector_only_score(
    vector_sim: f32,
    trust: TrustState,
    updated_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f32 {
    let tw = trust_weight(trust);
    let rw = recency_weight(updated_at, now);
    let combined = vector_sim + GAMMA * tw + DELTA * rw;
    combined * tw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_weight_ordering() {
        assert!(trust_weight(TrustState::Verified) > trust_weight(TrustState::Reviewed));
        assert!(trust_weight(TrustState::Reviewed) > trust_weight(TrustState::Draft));
        assert!(trust_weight(TrustState::Draft) > trust_weight(TrustState::Stale));
        assert!((trust_weight(TrustState::Archived) - 0.0).abs() < f32::EPSILON);
        assert!((trust_weight(TrustState::Rejected) - 0.0).abs() < f32::EPSILON);
        assert!((trust_weight(TrustState::Verified) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn weights_sum_to_one() {
        let sum = ALPHA + BETA + GAMMA + DELTA;
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "weights must sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn bm25_normalization_bounded() {
        assert!((normalize_bm25(0.0) - 1.0).abs() < f32::EPSILON);
        let n = normalize_bm25(-3.0);
        assert!((0.0..=1.0).contains(&n));
        let huge = normalize_bm25(-1e9);
        assert!((0.0..=1.0).contains(&huge));
    }

    #[test]
    fn recency_decay() {
        let now = chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let same = recency_weight(now, now);
        assert!((same - 1.0).abs() < 1e-4);
        let week_ago = now - chrono::Duration::days(7);
        assert!(recency_weight(week_ago, now) < same);
        let year_ago = now - chrono::Duration::days(365);
        assert!(recency_weight(year_ago, now) < recency_weight(week_ago, now));
    }
}
