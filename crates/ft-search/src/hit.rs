//! Result type for [`crate::SearchEngine::search`].

use ft_core::{RecordId, RecordKind, TrustState};

/// Which ranking pipeline produced a particular hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitMode {
    /// Hit came from FTS5 lexical matching only.
    Lexical,
    /// Hit came from vector similarity only.
    Vector,
    /// Hit was scored by combining both signals.
    Hybrid,
}

/// One ranked hit returned by [`crate::SearchEngine::search`].
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Canonical record id.
    pub id: RecordId,
    /// Record kind, surfaced so callers don't need a follow-up index lookup.
    pub kind: RecordKind,
    /// Short title (mirrors `records.title`).
    pub title: String,
    /// Final ranking score, after trust + recency multipliers. Higher = better.
    pub score: f32,
    /// Trust state at index time.
    pub trust: TrustState,
    /// Which signal produced this hit.
    pub mode: HitMode,
}
