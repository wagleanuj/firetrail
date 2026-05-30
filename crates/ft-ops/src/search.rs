//! Cross-domain (unified) search op.
//!
//! Where [`crate::memory::search`] is hard-scoped to the six memory kinds,
//! this op searches across **every** indexed [`ft_search::IndexKind`] —
//! work-tracking records (epic / task / subtask / bug), memory kinds, and the
//! synthetic domains (scope / identity / audit). It is the engine behind the
//! web Cmd+K global palette.
//!
//! The op is a thin wrapper over the `ft_search` engine: it builds a
//! [`SearchQuery`] with an arbitrary `kind_filter`, runs the same
//! daemon-first / mock-fallback embedding strategy as the memory search op,
//! applies the ADR-0014 quarantine filter, and surfaces `kind` + `scope` +
//! `trust` on every hit so callers can render badges and route to the record.
//!
//! ft-ui (and any other adapter) calls this instead of reaching into
//! `ft_search` directly, keeping the layering identical to the memory surface.

use ft_embed::Embedder as _;
use ft_embed::{DaemonStatus, MockEmbedder, daemon as embed_daemon};
use ft_import::is_quarantined;
use ft_core::RecordKind;
use ft_search::{
    EMBEDDING_DIM, HitMode, IndexKind, SearchHit, SearchMode as CoreSearchMode, SearchQuery,
};
use ft_storage::Storage as _;
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::memory::{SearchMode, TrustStateInput};
use crate::workspace::Workspace;

/// A search-layer kind the caller can filter on. Mirrors
/// [`ft_search::IndexKind`] as a flat, ts-rs-exportable enum (the record
/// variants are inlined so the wire shape is a single string union, matching
/// the lowercase labels the engine already uses).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    /// Epic work item.
    Epic,
    /// Task work item.
    Task,
    /// Subtask work item.
    Subtask,
    /// Bug work item.
    Bug,
    /// Incident memory record.
    Incident,
    /// Finding memory record.
    Finding,
    /// Runbook memory record.
    Runbook,
    /// Decision memory record.
    Decision,
    /// Gotcha memory record.
    Gotcha,
    /// Generic memory note.
    Memory,
    /// File-backed doc record.
    Doc,
    /// Scope definition (synthetic).
    Scope,
    /// Registered identity (synthetic).
    Identity,
    /// Audit/history entry (synthetic).
    Audit,
}

impl SearchKind {
    fn to_index_kind(self) -> IndexKind {
        match self {
            Self::Epic => IndexKind::Record(RecordKind::Epic),
            Self::Task => IndexKind::Record(RecordKind::Task),
            Self::Subtask => IndexKind::Record(RecordKind::Subtask),
            Self::Bug => IndexKind::Record(RecordKind::Bug),
            Self::Incident => IndexKind::Record(RecordKind::Incident),
            Self::Finding => IndexKind::Record(RecordKind::Finding),
            Self::Runbook => IndexKind::Record(RecordKind::Runbook),
            Self::Decision => IndexKind::Record(RecordKind::Decision),
            Self::Gotcha => IndexKind::Record(RecordKind::Gotcha),
            Self::Memory => IndexKind::Record(RecordKind::Memory),
            Self::Doc => IndexKind::Record(RecordKind::Doc),
            Self::Scope => IndexKind::Scope,
            Self::Identity => IndexKind::Identity,
            Self::Audit => IndexKind::Audit,
        }
    }
}

/// Input for [`search`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchInput {
    /// Free-text query string.
    pub query: String,
    /// Mode. Defaults to [`SearchMode::Auto`].
    #[serde(default)]
    pub mode: SearchMode,
    /// Minimum trust floor.
    #[serde(default)]
    pub trust: Option<TrustStateInput>,
    /// Restrict to these search kinds. Empty means "all kinds".
    #[serde(default)]
    pub kinds: Vec<SearchKind>,
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

/// One ranked cross-domain hit. Unlike [`crate::memory::SearchHitOut`] this
/// carries the owning `scope` so the global palette can show + route by it.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchHit {
    /// Document id (record id, or `<tag>:<key>` for synthetic docs).
    pub id: String,
    /// Document kind (lowercase label, e.g. `task` / `memory` / `scope`).
    pub kind: String,
    /// Title.
    pub title: String,
    /// Ranking score (higher is better).
    pub score: f32,
    /// Trust state (lowercase).
    pub trust: String,
    /// Owning scope, if any.
    pub scope: Option<String>,
    /// Which signal produced this hit (`"lexical" | "vector" | "hybrid"`).
    pub mode: String,
    /// Quarantine marker (only `true` for imported-but-unpromoted records).
    #[serde(default)]
    pub quarantine: bool,
}

impl From<SearchHit> for GlobalSearchHit {
    fn from(h: SearchHit) -> Self {
        Self {
            id: h.id.as_storage_str(),
            kind: h.kind.label().to_string(),
            title: h.title,
            score: h.score,
            trust: serialize_lower(&h.trust),
            scope: h.owning_scope,
            mode: hit_mode_label(h.mode).to_string(),
            quarantine: false,
        }
    }
}

/// Output of [`search`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchOutput {
    /// Resolved mode label (after any degradation).
    pub mode: String,
    /// Ranked hits, highest score first.
    pub hits: Vec<GlobalSearchHit>,
    /// Non-fatal warnings (e.g. "vector search unavailable; degraded to
    /// lexical").
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Cross-domain `search` op.
///
/// Embedding policy matches [`crate::memory::search`]: vector/hybrid modes try
/// a daemon that is **already running** (auto-spawn via the same helper), and
/// fall back to a deterministic mock embedder rather than erroring, so the op
/// degrades to lexical when no model is installed. It never blocks on spawning
/// a daemon from an HTTP request beyond the existing `ensure_running` policy.
pub fn search(
    ws: &Workspace,
    identity: &Identity,
    input: GlobalSearchInput,
    _events: &EventBus,
) -> Result<GlobalSearchOutput, OpsError> {
    let mut ctx = crate::memory::ctx_for_trust(ws, identity, "search")?;
    let mut warnings: Vec<String> = Vec::new();

    let mode = input.mode.to_core_mode();
    let want_embedding = matches!(mode, CoreSearchMode::Hybrid | CoreSearchMode::Vector);

    let embedding = if want_embedding {
        match crate::memory::ensure_daemon_running(ws)? {
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
                        warnings
                            .push(format!("mock embedder failed ({e}); falling back to lexical"));
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
        query.kind_filter = input.kinds.iter().map(|k| k.to_index_kind()).collect();
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

    // Quarantine filter (ADR-0014). Synthetic docs never carry a record id and
    // are always kept.
    let hit_views: Vec<GlobalSearchHit> = hits
        .into_iter()
        .filter_map(|h| {
            let quarantined = match h.id.as_record_id() {
                Some(rid) => ctx.storage.read(rid).is_ok_and(|rec| is_quarantined(&rec)),
                None => false,
            };
            if quarantined && !input.include_quarantine {
                return None;
            }
            let mut view = GlobalSearchHit::from(h);
            if quarantined {
                view.quarantine = true;
            }
            Some(view)
        })
        .collect();

    Ok(GlobalSearchOutput {
        mode: mode_label(resolved_mode).to_string(),
        hits: hit_views,
        warnings,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers (mirrors crate::memory::search; kept local so the surfaces stay
// independent and the memory helpers can stay private).
// ─────────────────────────────────────────────────────────────────────────────

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
