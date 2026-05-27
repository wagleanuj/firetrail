//! Pack data types.

use ft_core::{RecordId, RecordKind, TrustState};
use serde::{Deserialize, Serialize};

/// A bounded, deterministic bundle of records selected for an agent to read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    /// Target record id, if this pack was built for a specific task.
    pub target_id: Option<RecordId>,
    /// Query string, if this pack was built from a free-form query.
    pub query: Option<String>,
    /// Selected items, highest-priority first.
    pub items: Vec<PackItem>,
    /// Estimated total token cost of the included items.
    pub total_tokens: usize,
    /// Budget the pack was constrained to.
    pub budget: usize,
    /// Records that were considered but dropped, with the reason they were
    /// dropped.
    pub omitted: Vec<OmittedEntry>,
}

/// One record included in a [`ContextPack`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackItem {
    /// Record id.
    pub id: RecordId,
    /// Record kind.
    pub kind: RecordKind,
    /// Record title.
    pub title: String,
    /// Trust state of the source record (defaults to `Verified` for non-memory
    /// kinds, which carry no trust field of their own).
    pub trust: TrustState,
    /// Priority score (higher is better).
    pub score: f32,
    /// Estimated token count of this item's rendered form.
    pub tokens: usize,
    /// Body excerpt, possibly truncated. Truncated excerpts end in the
    /// sentinel `"...truncated..."`.
    pub body_excerpt: String,
}

/// One omitted-record entry recorded in [`ContextPack::omitted`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmittedEntry {
    /// Record id.
    pub id: RecordId,
    /// Record kind.
    pub kind: RecordKind,
    /// Why the record was dropped.
    pub reason: OmittedReason,
}

/// Reason a candidate record was excluded from the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmittedReason {
    /// Including this record would have exceeded `max_tokens`.
    BudgetExceeded,
    /// Record was older than the configured staleness cutoff.
    TooStale,
    /// Record's trust state was below the configured trust floor.
    BelowTrustFloor,
    /// Record did not match the configured scope or kind filter.
    ScopeFiltered,
}
