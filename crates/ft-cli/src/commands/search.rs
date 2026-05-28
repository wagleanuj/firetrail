//! `firetrail search` and `firetrail similar` — M3 search surface.

use ft_embed::{Embedder, MockEmbedder, daemon};
use ft_import::is_quarantined;
use ft_search::{HitMode, SearchHit, SearchMode, SearchQuery};
use ft_storage::Storage as _;
use serde::Serialize;

use crate::cli::{EmbedderArg, GlobalOpts, SearchArgs, SimilarArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_SEARCH: &str = "search";
const CMD_SIMILAR: &str = "similar";

/// `firetrail search <query>`
pub fn search(args: &SearchArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_SEARCH, global.workspace.as_deref())?;
    let mut warnings = ctx.warnings.clone();

    let mode = args.mode.to_core();
    let want_embedding = matches!(mode, SearchMode::Hybrid | SearchMode::Vector);

    // Compute / fetch the query embedding when the requested mode benefits
    // from one. The default `MockEmbedder` is deterministic and good enough
    // for M3 — real ONNX is a follow-up.
    let embedding = if want_embedding {
        match args.embedder {
            EmbedderArg::Mock => {
                let embedder = MockEmbedder::new(0, ft_search::EMBEDDING_DIM);
                match embedder.embed(&args.query) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warnings.push(format!("mock embedder failed: {e}"));
                        None
                    }
                }
            }
            EmbedderArg::Daemon => {
                let socket = ctx.ws.daemon_socket_path();
                // Auto-spawn a detached daemon if none is listening (firetrail-e7z).
                if let Err(e) = crate::commands::daemon_cmd::ensure_running(CMD_SEARCH, &ctx.ws) {
                    warnings.push(format!("could not auto-spawn daemon: {e}"));
                }
                match daemon::send_embed(&socket, &args.query) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warnings.push(format!(
                            "daemon embedder unavailable ({e}); falling back to lexical"
                        ));
                        None
                    }
                }
            }
        }
    } else {
        None
    };

    // ADR-0007 lexical fallback: if the caller asked for Vector or Hybrid
    // but we could not obtain an embedding (mock failed, daemon down), the
    // actual ranking degrades to lexical-only. Reflect that in both the
    // engine query and the outcome `mode` field so callers see what really
    // happened (firetrail-urq).
    let resolved_mode = match (mode, embedding.is_some()) {
        (SearchMode::Vector | SearchMode::Hybrid, false) => SearchMode::Lexical,
        (m, _) => m,
    };

    let mut query = SearchQuery {
        text: args.query.clone(),
        mode: resolved_mode,
        embedding,
        limit: args.limit.max(1),
        ..SearchQuery::default()
    };
    if let Some(t) = args.trust {
        query.min_trust = Some(t.to_core());
    }
    if !args.kinds.is_empty() {
        query.kind_filter = args.kinds.iter().map(|k| k.to_core()).collect();
    }
    if let Some(s) = &args.scope {
        query.scope_filter = Some(s.clone());
    }

    let hits = {
        let engine = ctx.search_engine()?;
        engine
            .search(&query)
            .map_err(|e| CliError::internal(CMD_SEARCH, format!("search: {e}")))?
    };

    // Quarantine filter (ADR-0014). By default, imported/quarantined records
    // are dropped from the result set; with `--include-quarantine` they pass
    // through with a `quarantine: true` marker in the JSON view. We materialise
    // the filter at the CLI layer (rather than in ft-search) so the search
    // engine stays oblivious to import labels — see firetrail-2z2.
    let hit_views: Vec<SearchHitView> = hits
        .into_iter()
        .filter_map(|h| {
            let quarantined = match ctx.storage.read(&h.id) {
                Ok(rec) => is_quarantined(&rec),
                Err(_) => false,
            };
            if quarantined && !args.include_quarantine {
                return None;
            }
            let mut view = SearchHitView::from(h);
            if quarantined {
                view.quarantine = true;
            }
            Some(view)
        })
        .collect();

    Ok(CommandOutcome::Search(SearchOutcome {
        command: CMD_SEARCH,
        query: args.query.clone(),
        mode: mode_label(resolved_mode),
        hits: hit_views,
        warnings,
    }))
}

/// `firetrail similar <id>`
pub fn similar(args: &SimilarArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_SIMILAR, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let id = ctx.resolve_id(&args.id)?;
    let engine = ctx.search_engine()?;
    let hits = engine
        .similar(&id, args.limit.max(1))
        .map_err(|e| CliError::internal(CMD_SIMILAR, format!("similar: {e}")))?;
    Ok(CommandOutcome::Search(SearchOutcome {
        command: CMD_SIMILAR,
        query: id.as_str().to_string(),
        mode: "similar".to_string(),
        hits: hits.into_iter().map(SearchHitView::from).collect(),
        warnings,
    }))
}

fn mode_label(mode: SearchMode) -> String {
    match mode {
        SearchMode::Auto => "auto",
        SearchMode::Lexical => "lexical",
        SearchMode::Hybrid => "hybrid",
        SearchMode::Vector => "vector",
    }
    .to_string()
}

fn hit_mode_label(mode: HitMode) -> &'static str {
    match mode {
        HitMode::Lexical => "lexical",
        HitMode::Vector => "vector",
        HitMode::Hybrid => "hybrid",
    }
}

/// JSON / markdown view of a single search hit.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHitView {
    /// Record id.
    pub id: String,
    /// Record kind (lowercase string).
    pub kind: String,
    /// Title.
    pub title: String,
    /// Ranking score (higher is better).
    pub score: f32,
    /// Trust state (serde label).
    pub trust: String,
    /// Which signal produced this hit.
    pub mode: &'static str,
    /// Marker for quarantined imports (only present when the hit refers to a
    /// record carrying the `quarantine=true` label and the caller passed
    /// `--include-quarantine`).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub quarantine: bool,
}

impl From<SearchHit> for SearchHitView {
    fn from(h: SearchHit) -> Self {
        Self {
            id: h.id.as_str().to_string(),
            kind: serde_lower(&h.kind),
            title: h.title,
            score: h.score,
            trust: serde_lower(&h.trust),
            mode: hit_mode_label(h.mode),
            quarantine: false,
        }
    }
}

fn serde_lower<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Outcome of `firetrail search` / `firetrail similar`.
#[derive(Debug, Clone, Serialize)]
pub struct SearchOutcome {
    /// Stable command name for the JSON envelope.
    #[serde(skip)]
    pub command: &'static str,
    /// The query string (or source record id for `similar`).
    pub query: String,
    /// Resolved mode label.
    pub mode: String,
    /// Ranked hits, highest score first.
    pub hits: Vec<SearchHitView>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl SearchOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        if self.hits.is_empty() {
            return "_no matches_\n".to_string();
        }
        let mut s = String::new();
        let _ = writeln!(
            s,
            "**{}** `{}` — {} hit(s) [{}]",
            self.command,
            self.query,
            self.hits.len(),
            self.mode
        );
        s.push_str("\n| ID | Kind | Score | Trust | Mode | Title |\n");
        s.push_str("|----|------|-------|-------|------|-------|\n");
        for h in &self.hits {
            let _ = writeln!(
                s,
                "| `{}` | {} | {:.3} | {} | {} | {} |",
                h.id,
                h.kind,
                h.score,
                h.trust,
                h.mode,
                h.title.replace('|', "\\|"),
            );
        }
        s
    }
    /// One-line quiet summary.
    pub fn quiet_line(&self) -> String {
        format!("{}: {} hit(s)", self.command, self.hits.len())
    }
}
