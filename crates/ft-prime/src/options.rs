//! Caller-facing options for [`crate::prime_for_task`] and
//! [`crate::prime_for_query`].

use chrono::{DateTime, Utc};
use ft_core::{RecordKind, TrustState};

/// Output format selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrimeFormat {
    /// Markdown text format. Default budget per ADR-0019: 8,000 tokens.
    #[default]
    Markdown,
    /// Structured JSON format. Default budget per ADR-0019: 16,000 tokens.
    Json,
}

/// Tuning knobs for prime generation.
///
/// `now` is injected so tests are deterministic; production callers normally
/// set it to [`Utc::now`].
#[derive(Debug, Clone)]
pub struct PrimeOptions {
    /// Token budget for the produced pack. Default `8000`.
    pub max_tokens: usize,
    /// If set, drop records whose trust ranks below this floor.
    pub min_trust: Option<TrustState>,
    /// If non-empty, only consider records of these kinds.
    pub kind_filter: Vec<RecordKind>,
    /// If set, only consider records whose `owning_scope` equals this string.
    pub scope_filter: Option<String>,
    /// Output format the pack is intended to be rendered as.
    pub format: PrimeFormat,
    /// Logical "now" used for recency scoring. Tests pin this to a fixed
    /// timestamp so output is deterministic.
    pub now: DateTime<Utc>,
}

impl Default for PrimeOptions {
    fn default() -> Self {
        Self {
            max_tokens: 8000,
            min_trust: None,
            kind_filter: Vec::new(),
            scope_filter: None,
            format: PrimeFormat::Markdown,
            now: Utc::now(),
        }
    }
}
