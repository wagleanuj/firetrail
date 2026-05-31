//! `/api/search` — unified cross-domain search HTTP surface.
//!
//! | Method | Path          | Op                       |
//! |--------|---------------|--------------------------|
//! | GET    | `/api/search` | [`ft_ops::search::search`] |
//!
//! Unlike `/api/memory/search` (hard-scoped to memory kinds), this route
//! searches across **every** indexed kind — work-tracking records
//! (epic/task/subtask/bug), memory kinds, and the synthetic domains
//! (scope/identity/audit). It is a thin adapter: the actual cross-kind query,
//! embedding strategy, and quarantine filtering all live in the ft-ops search
//! op, mirroring how `memory.rs` delegates to `ft_ops::memory::search`.
//!
//! Query shape: `?q=&kind=&mode=&trust=&scope=&limit=&includeQuarantine=`.
//! `kind` may be repeated to filter on several kinds at once
//! (`?kind=task&kind=memory`); omitting it searches all kinds.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use ft_ops::search::{self, GlobalSearchInput, SearchKind};
use ft_ops::memory::{SearchMode, TrustStateInput};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

use super::tickets::{REQUEST_ID_HEADER, resolve_identity};

/// Maximum `limit` accepted on the search route.
const SEARCH_LIMIT_MAX: usize = 100;

/// Build the `/api/search` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(search_handler))
}

fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}

/// Query params for `GET /api/search`.
///
/// `kind` is a comma-separated list (`?kind=task,memory,scope`). A
/// comma-joined string is used rather than repeated params because
/// `axum::Query` (`serde_urlencoded`) cannot deserialize repeated keys into a
/// `Vec` without an extra dependency. Omitting it searches all kinds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchQuery {
    /// Free-text query.
    #[serde(alias = "query")]
    pub q: String,
    /// Search mode (`auto | lexical | vector | hybrid`).
    #[serde(default)]
    pub mode: Option<SearchMode>,
    /// Comma-separated kinds to filter on (e.g. `task,memory`). Empty / absent
    /// → search all kinds.
    #[serde(default)]
    pub kind: Option<String>,
    /// Minimum trust floor.
    #[serde(default)]
    pub trust: Option<TrustStateInput>,
    /// Restrict to owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap hits (1..=100). Defaults to 20.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Include quarantined records.
    #[serde(default)]
    pub include_quarantine: bool,
}

/// `GET /api/search` — unified cross-domain search.
#[tracing::instrument(skip_all)]
pub async fn search_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<GlobalSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(n) = q.limit {
        if n == 0 || n > SEARCH_LIMIT_MAX {
            return Err(AppError::Ops(ft_ops::OpsError::validation(
                "limit",
                format!("must be in 1..={SEARCH_LIMIT_MAX}"),
            )));
        }
    }
    let kinds = parse_kinds(q.kind.as_deref())?;
    let identity = resolve_identity(&state.workspace)?;
    let input = GlobalSearchInput {
        query: q.q,
        mode: q.mode.unwrap_or_default(),
        trust: q.trust,
        kinds,
        scope: q.scope,
        limit: q.limit.unwrap_or(20),
        include_quarantine: q.include_quarantine,
        request_id: request_id(&headers),
    };
    // `search::search` is synchronous and can block for seconds (embedding /
    // embed-daemon spawn). Run it on the blocking pool so a slow search never
    // ties up an async worker thread — and so a burst of debounced semantic
    // searches can't starve the runtime and stall every other request
    // (firetrail-paag).
    let out = tokio::task::spawn_blocking(move || {
        search::search(&state.workspace, &identity, input, &state.events)
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("search task failed: {e}")))??;
    Ok((StatusCode::OK, Json(out)))
}

/// Parse a comma-separated `kind` query value into a `Vec<SearchKind>`.
/// Each token is deserialized through `SearchKind`'s own serde labels so the
/// accepted values stay in lockstep with the generated TS union. An unknown
/// label is a 400 rather than a silent drop.
fn parse_kinds(raw: Option<&str>) -> Result<Vec<SearchKind>, AppError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let kind: SearchKind = serde_json::from_value(serde_json::Value::String(token.to_string()))
            .map_err(|_| {
                AppError::Ops(ft_ops::OpsError::validation(
                    "kind",
                    format!("unknown search kind `{token}`"),
                ))
            })?;
        out.push(kind);
    }
    Ok(out)
}
