//! Memory search ops: `search` (lexical / hybrid / vector) and `similar`.
//!
//! Mirrors `ft_cli::commands::search` but stripped of clap and CLI-specific
//! envelope types. When the requested mode benefits from an embedding the
//! op auto-spawns the embed-daemon via a private helper; if the daemon
//! is unreachable the op degrades to lexical search and surfaces a
//! warning in [`SearchOutput::warnings`].
//!
//! Quarantine filtering (ADR-0014) is applied at this layer so the search
//! engine remains oblivious to import labels.

use ft_embed::Embedder as _;
use ft_embed::{DaemonStatus, MockEmbedder, daemon as embed_daemon};
use ft_import::is_quarantined;
use ft_search::{EMBEDDING_DIM, HitMode, SearchHit, SearchMode as CoreSearchMode, SearchQuery};
use ft_storage::Storage as _;
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::create::MemoryKind;
use super::ctx::MemoryCtx;
use super::views::TrustStateInput;

/// Wire shape for the requested search mode.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Engine chooses: hybrid when embedding is available, lexical otherwise.
    #[default]
    Auto,
    /// FTS5 lexical only.
    Lexical,
    /// Vector only (requires a query embedding + sqlite-vec).
    Vector,
    /// Vector + lexical weighted-sum rank.
    Hybrid,
}

impl SearchMode {
    fn to_core(self) -> CoreSearchMode {
        match self {
            Self::Auto => CoreSearchMode::Auto,
            Self::Lexical => CoreSearchMode::Lexical,
            Self::Vector => CoreSearchMode::Vector,
            Self::Hybrid => CoreSearchMode::Hybrid,
        }
    }
}

/// Input for [`search`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchInput {
    /// Free-text query string.
    pub query: String,
    /// Mode. Defaults to [`SearchMode::Auto`].
    #[serde(default)]
    pub mode: SearchMode,
    /// Minimum trust floor.
    #[serde(default)]
    pub trust: Option<TrustStateInput>,
    /// Restrict to memory kinds. Empty means "all kinds".
    #[serde(default)]
    pub kinds: Vec<MemoryKind>,
    /// Restrict to owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap the number of hits. Defaults to 20.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Include quarantined (imported but not yet promoted) records.
    #[serde(default)]
    pub include_quarantine: bool,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn default_limit() -> usize {
    20
}

/// Input for [`similar`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarInput {
    /// Source record id (full or unambiguous prefix).
    pub id: String,
    /// Cap the number of hits. Defaults to 10.
    #[serde(default = "default_similar_limit")]
    pub limit: usize,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn default_similar_limit() -> usize {
    10
}

/// One ranked hit.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitOut {
    /// Record id.
    pub id: String,
    /// Record kind (lowercase).
    pub kind: String,
    /// Title.
    pub title: String,
    /// Ranking score (higher is better).
    pub score: f32,
    /// Trust state (lowercase).
    pub trust: String,
    /// Which signal produced this hit (`"lexical" | "vector" | "hybrid"`).
    pub mode: String,
    /// Quarantine marker (only `true` for imported-but-unpromoted records).
    #[serde(default)]
    pub quarantine: bool,
}

impl From<SearchHit> for SearchHitOut {
    fn from(h: SearchHit) -> Self {
        Self {
            id: h.id.as_str().to_string(),
            kind: serialize_lower(&h.kind),
            title: h.title,
            score: h.score,
            trust: serialize_lower(&h.trust),
            mode: hit_mode_label(h.mode).to_string(),
            quarantine: false,
        }
    }
}

/// Output of [`search`] / [`similar`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOutput {
    /// Resolved mode label (after any degradation).
    pub mode: String,
    /// Ranked hits, highest score first.
    pub hits: Vec<SearchHitOut>,
    /// Non-fatal warnings (e.g. "vector search unavailable; degraded to
    /// lexical").
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// `search` op.
pub fn search(
    ws: &Workspace,
    identity: &Identity,
    input: SearchInput,
    _events: &EventBus,
) -> Result<SearchOutput, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "search")?;
    let mut warnings: Vec<String> = Vec::new();

    let mode = input.mode.to_core();
    let want_embedding = matches!(mode, CoreSearchMode::Hybrid | CoreSearchMode::Vector);

    // Embedding strategy: try the daemon (auto-spawning if needed) for any
    // vector-flavoured request. If the daemon is unreachable, fall back to
    // the deterministic MockEmbedder so vector mode still produces output
    // in test environments without a model on disk.
    let embedding = if want_embedding {
        match super::daemon::ensure_running(ws)? {
            DaemonStatus::Running => match ws.daemon_socket_path() {
                Ok(socket) => match embed_daemon::send_embed(&socket, &input.query) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warnings.push(format!(
                            "daemon embedder unavailable ({e}); falling back to lexical"
                        ));
                        None
                    }
                },
                Err(e) => {
                    warnings.push(format!(
                        "could not resolve daemon socket ({e}); falling back to lexical"
                    ));
                    None
                }
            },
            DaemonStatus::Stopped | DaemonStatus::Unreachable => {
                // Mock fallback keeps semantic / hybrid producing
                // deterministic results when the operator has no model
                // installed. The CLI used the same MockEmbedder when
                // --embedder=mock.
                let embedder = MockEmbedder::new(0, EMBEDDING_DIM);
                match embedder.embed(&input.query) {
                    Ok(v) => {
                        warnings.push(
                            "embed-daemon unavailable; using deterministic mock embedder"
                                .to_string(),
                        );
                        Some(v)
                    }
                    Err(e) => {
                        warnings.push(format!(
                            "mock embedder failed ({e}); falling back to lexical"
                        ));
                        None
                    }
                }
            }
        }
    } else {
        None
    };

    let vector_enabled = ctx.read_search_engine()?.vector_enabled();
    let resolved_mode =
        resolve_search_mode(mode, embedding.is_some(), vector_enabled, &mut warnings);

    let mut query = SearchQuery {
        text: input.query.clone(),
        mode: resolved_mode,
        embedding,
        limit: input.limit.max(1),
        ..SearchQuery::default()
    };
    if let Some(t) = input.trust {
        query.min_trust = Some(t.to_core_for_search());
    }
    if !input.kinds.is_empty() {
        query.kind_filter = input.kinds.iter().map(|k| k.to_core()).collect();
    }
    if let Some(s) = input.scope {
        query.scope_filter = Some(s);
    }

    let hits = {
        let engine = ctx.read_search_engine()?;
        engine
            .search(&query)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("search: {e}")))?
    };

    // Quarantine filter (ADR-0014).
    let hit_views: Vec<SearchHitOut> = hits
        .into_iter()
        .filter_map(|h| {
            let quarantined = match ctx.storage.read(&h.id) {
                Ok(rec) => is_quarantined(&rec),
                Err(_) => false,
            };
            if quarantined && !input.include_quarantine {
                return None;
            }
            let mut view = SearchHitOut::from(h);
            if quarantined {
                view.quarantine = true;
            }
            Some(view)
        })
        .collect();

    Ok(SearchOutput {
        mode: mode_label(resolved_mode).to_string(),
        hits: hit_views,
        warnings,
    })
}

/// `similar` op.
#[allow(clippy::needless_pass_by_value)]
pub fn similar(
    ws: &Workspace,
    identity: &Identity,
    input: SimilarInput,
    _events: &EventBus,
) -> Result<SearchOutput, OpsError> {
    let mut ctx = MemoryCtx::open(ws, identity, "similar")?;
    let id = ctx.resolve_id(&input.id)?;
    let engine = ctx.read_search_engine()?;
    let hits = engine
        .similar(&id, input.limit.max(1))
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("similar: {e}")))?;
    Ok(SearchOutput {
        mode: "similar".to_string(),
        hits: hits.into_iter().map(SearchHitOut::from).collect(),
        warnings: Vec::new(),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers.
// ─────────────────────────────────────────────────────────────────────────────

impl TrustStateInput {
    fn to_core_for_search(self) -> ft_core::TrustState {
        // Re-use the existing conversion; we cannot call the private `to_core`
        // from another module, so go via the JSON serialization round-trip.
        // (Kept inlined here rather than promoting `to_core` to pub(crate) so
        // the surface stays narrow.)
        match self {
            Self::Draft => ft_core::TrustState::Draft,
            Self::Reviewed => ft_core::TrustState::Reviewed,
            Self::Verified => ft_core::TrustState::Verified,
            Self::Stale => ft_core::TrustState::Stale,
            Self::Deprecated => ft_core::TrustState::Deprecated,
            Self::Archived => ft_core::TrustState::Archived,
            Self::Superseded => ft_core::TrustState::Superseded,
            Self::Rejected => ft_core::TrustState::Rejected,
            Self::Redacted => ft_core::TrustState::Redacted,
        }
    }
}

fn resolve_search_mode(
    requested: CoreSearchMode,
    has_embedding: bool,
    vector_enabled: bool,
    warnings: &mut Vec<String>,
) -> CoreSearchMode {
    match (requested, has_embedding, vector_enabled) {
        (CoreSearchMode::Vector, _, false) => {
            warnings.push(
                "vector search unavailable (sqlite-vec disabled); falling back to lexical"
                    .to_string(),
            );
            CoreSearchMode::Lexical
        }
        (CoreSearchMode::Vector, false, _) => {
            warnings.push(
                "vector search unavailable (no query embedding); falling back to lexical"
                    .to_string(),
            );
            CoreSearchMode::Lexical
        }
        (CoreSearchMode::Hybrid, false, _) => CoreSearchMode::Lexical,
        (m, _, _) => m,
    }
}

fn mode_label(mode: CoreSearchMode) -> &'static str {
    match mode {
        CoreSearchMode::Auto => "auto",
        CoreSearchMode::Lexical => "lexical",
        CoreSearchMode::Hybrid => "hybrid",
        CoreSearchMode::Vector => "vector",
    }
}

fn hit_mode_label(mode: HitMode) -> &'static str {
    match mode {
        HitMode::Lexical => "lexical",
        HitMode::Vector => "vector",
        HitMode::Hybrid => "hybrid",
    }
}

fn serialize_lower<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
