//! Result type for [`crate::SearchEngine::search`].

use ft_core::TrustState;

use crate::kind::{DocId, IndexKind};

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
    /// Document id (record or synthetic).
    pub id: DocId,
    /// Document kind.
    pub kind: IndexKind,
    /// Short title.
    pub title: String,
    /// Final ranking score, after trust + recency multipliers. Higher = better.
    pub score: f32,
    /// Trust state at index time.
    pub trust: TrustState,
    /// Owning scope of the indexed document, if any. `None` for synthetic docs
    /// that carry no scope (and for records written before the column existed).
    pub owning_scope: Option<String>,
    /// Which signal produced this hit.
    pub mode: HitMode,
}
