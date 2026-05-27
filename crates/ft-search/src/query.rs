//! Query and mode types for [`crate::SearchEngine::search`].

use ft_core::{RecordKind, TrustState};

/// Which signal(s) the caller wants the engine to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Engine chooses: hybrid when an embedding is supplied and the
    /// `sqlite-vec` extension is loaded, lexical otherwise.
    #[default]
    Auto,
    /// Force FTS5-only ranking.
    Lexical,
    /// Force vector-only ranking. Returns an error if the engine has no
    /// vector support compiled in or no embedding was provided.
    Vector,
    /// Combine vector + lexical signals using the weighted-sum rank.
    Hybrid,
}

/// Default page size for [`crate::SearchEngine::search`].
pub const DEFAULT_LIMIT: usize = 20;

/// One search request.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Free-text query string. Used directly as the FTS5 `MATCH` argument.
    pub text: String,
    /// Which ranking signal(s) to use.
    pub mode: SearchMode,
    /// Drop hits whose trust state is strictly below this threshold.
    ///
    /// `None` means "no trust filter" (drafts included).
    pub min_trust: Option<TrustState>,
    /// Restrict to these kinds. Empty means "all kinds".
    pub kind_filter: Vec<RecordKind>,
    /// Restrict to this owning scope. `None` means "any scope".
    pub scope_filter: Option<String>,
    /// Maximum number of hits to return.
    pub limit: usize,
    /// Pre-computed query embedding. `None` forces lexical-only behaviour
    /// regardless of [`Self::mode`] (except when `mode == Vector`, which then
    /// errors).
    pub embedding: Option<Vec<f32>>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            mode: SearchMode::default(),
            min_trust: None,
            kind_filter: Vec::new(),
            scope_filter: None,
            limit: DEFAULT_LIMIT,
            embedding: None,
        }
    }
}

impl SearchQuery {
    /// Construct a query from a free-text string with all other fields default.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}
