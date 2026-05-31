//! `/api/memory` HTTP surface — Wave 2 Batch B of the firetrail GUI.
//!
//! Route table:
//!
//! | Method | Path                          | Op                                |
//! |--------|-------------------------------|-----------------------------------|
//! | GET    | `/api/memory`                 | [`ft_ops::memory::views::list`]   |
//! | GET    | `/api/memory/stale`           | [`ft_ops::memory::views::stale`]  |
//! | GET    | `/api/memory/:id`             | [`ft_ops::memory::views::show`]   |
//! | POST   | `/api/memory`                 | discriminated create              |
//! | POST   | `/api/memory/capture`         | [`ft_ops::memory::create::capture`] |
//! | GET    | `/api/memory/search`          | [`ft_ops::memory::search::search`] |
//! | GET    | `/api/memory/similar/:id`     | [`ft_ops::memory::search::similar`] |
//! | POST   | `/api/memory/salvage`         | [`ft_ops::memory::salvage::salvage`] |
//!
//! ## Discriminated create
//!
//! `POST /api/memory` accepts a single JSON body tagged with `kind`:
//!
//! ```json
//! { "kind": "incident", "summary": "…" }
//! ```
//!
//! Variants: `"incident"`, `"finding"`, `"runbook"`, `"decision"`,
//! `"gotcha"`, `"memory"`. The `"memory"` variant is the generic
//! kind — explicit kinds always win when they match (priority order
//! is "literal discriminator wins"; there is no fallthrough).
//! For polymorphic "capture this blob as one of these kinds" semantics,
//! use `POST /api/memory/capture` instead, which carries `kind` inside
//! the body and falls back to `"memory"` when omitted.
//!
//! ## Salvage two-step
//!
//! [`SalvageInput::dry_run`] toggles between planning (returns candidates,
//! emits no events, writes nothing) and applying (mutates the repo,
//! emits one [`ft_ops::Event::MemorySalvaged`] per entry). The recommended
//! UI flow is `POST {dry_run: true}` → operator picks → `POST {dry_run:
//! false, selected: [...] }`.
//!
//! ## Search modes
//!
//! `?mode=` accepts `auto | lexical | vector | hybrid`. Vector- and
//! hybrid-flavoured requests auto-spawn the embed-daemon; if the daemon
//! is unreachable, the op degrades to lexical search and reports the
//! degradation in `warnings` — no 503 fallback is needed because the
//! ops layer already handles graceful degradation deterministically.
//!
//! ## Request id coalescing
//!
//! Every write accepts an optional `X-Firetrail-Request-Id` header. The
//! handler copies it onto the corresponding ops input's `request_id`
//! field; ops then emit through `EventBus::emit_with_request` so the
//! same client can filter its own SSE echoes.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::memory::{
    self, CaptureInput, CreateDecisionInput, CreateFindingInput, CreateGotchaInput,
    CreateIncidentInput, CreateMemoryInput, CreateRunbookInput, ListInput, MemoryKind,
    SalvageInput, SearchInput, SearchMode, ShowInput, SimilarInput, StaleInput, TrustStateInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

use super::tickets::REQUEST_ID_HEADER;

/// Maximum `limit` accepted on search routes.
const SEARCH_LIMIT_MAX: usize = 100;

/// Build the `/api/memory` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_handler).post(create_handler))
        .route("/stale", get(stale_handler))
        .route("/capture", post(capture_handler))
        .route("/search", get(search_handler))
        .route("/similar/:id", get(similar_handler))
        .route("/salvage", post(salvage_handler))
        // `/:id` is the catch-all; keep it last so the named routes above win.
        .route("/:id", get(show_handler))
}

// ─────────────────────────────────────────────────────────────────────────────
// Identity + request-id extraction (mirrors tickets.rs).
// ─────────────────────────────────────────────────────────────────────────────

fn resolve_identity(ws: &Workspace) -> Result<Identity, AppError> {
    let resolver = DefaultResolver::new(&ws.root, false);
    let core = resolver.resolve().map_err(|e| {
        AppError::Forbidden(format!(
            "identity unresolvable: {e} (set FIRETRAIL_AUTHOR or git config user.email)"
        ))
    })?;
    let s = core.as_str().to_string();
    Ok(Identity::new(s.clone(), s))
}

fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}

// ─────────────────────────────────────────────────────────────────────────────
// Read handlers.
// ─────────────────────────────────────────────────────────────────────────────

/// `GET /api/memory` — list memory records with optional filters.
#[tracing::instrument(skip_all)]
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
    Query(input): Query<ListInput>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = memory::views::list(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/memory/stale` — list records past their freshness window.
#[tracing::instrument(skip_all)]
pub async fn stale_handler(
    State(state): State<Arc<AppState>>,
    Query(input): Query<StaleInput>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = memory::views::stale(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/memory/:id` — full record (envelope + body).
#[tracing::instrument(skip_all)]
pub async fn show_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = memory::views::show(&state.workspace, &identity, ShowInput { id }, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Create.
// ─────────────────────────────────────────────────────────────────────────────

/// Discriminated create body for `POST /api/memory`.
///
/// Each variant carries the corresponding ops input verbatim (minus its
/// own `request_id`, which we fill from the header).
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CreateMemoryBody {
    /// Incident record.
    Incident(CreateIncidentInput),
    /// Finding record.
    Finding(CreateFindingInput),
    /// Runbook record.
    Runbook(CreateRunbookInput),
    /// Decision record.
    Decision(CreateDecisionInput),
    /// Gotcha record.
    Gotcha(CreateGotchaInput),
    /// Generic memory note.
    Memory(CreateMemoryInput),
}

/// `POST /api/memory` — discriminated create.
#[tracing::instrument(skip_all)]
pub async fn create_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateMemoryBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let rid = request_id(&headers);
    let out = match body {
        CreateMemoryBody::Incident(mut input) => {
            input.request_id = input.request_id.or(rid);
            memory::create_incident(&state.workspace, &identity, input, &state.events)?
        }
        CreateMemoryBody::Finding(mut input) => {
            input.request_id = input.request_id.or(rid);
            memory::create_finding(&state.workspace, &identity, input, &state.events)?
        }
        CreateMemoryBody::Runbook(mut input) => {
            input.request_id = input.request_id.or(rid);
            memory::create_runbook(&state.workspace, &identity, input, &state.events)?
        }
        CreateMemoryBody::Decision(mut input) => {
            input.request_id = input.request_id.or(rid);
            memory::create_decision(&state.workspace, &identity, input, &state.events)?
        }
        CreateMemoryBody::Gotcha(mut input) => {
            input.request_id = input.request_id.or(rid);
            memory::create_gotcha(&state.workspace, &identity, input, &state.events)?
        }
        CreateMemoryBody::Memory(mut input) => {
            input.request_id = input.request_id.or(rid);
            memory::create_memory(&state.workspace, &identity, input, &state.events)?
        }
    };
    Ok((StatusCode::CREATED, Json(out)))
}

/// `POST /api/memory/capture` — polymorphic capture op.
#[tracing::instrument(skip_all)]
pub async fn capture_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut input): Json<CaptureInput>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    input.request_id = input.request_id.or_else(|| request_id(&headers));
    let out = memory::capture(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::CREATED, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Search / similar.
// ─────────────────────────────────────────────────────────────────────────────

/// Query params for `GET /api/memory/search`.
///
/// `axum::Query` over `SearchInput` directly would refuse repeated `kind`
/// params and force the SPA to URL-encode a JSON blob. This wrapper accepts
/// the documented `?q=&mode=&kind=&limit=&trust=&scope=&includeQuarantine=`
/// shape and maps it onto the ops input.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    /// Free-text query.
    #[serde(alias = "query")]
    pub q: String,
    /// Search mode (`auto | lexical | vector | hybrid`).
    #[serde(default)]
    pub mode: Option<SearchMode>,
    /// Single memory kind (the GUI's UI only filters by one kind at a time;
    /// the ops layer accepts a `Vec<MemoryKind>` but we surface a singleton
    /// for query-string clarity).
    #[serde(default)]
    pub kind: Option<MemoryKind>,
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

/// `GET /api/memory/search` — keyword / semantic / hybrid search.
#[tracing::instrument(skip_all)]
pub async fn search_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(n) = q.limit {
        if n == 0 || n > SEARCH_LIMIT_MAX {
            return Err(AppError::Ops(ft_ops::OpsError::validation(
                "limit",
                format!("must be in 1..={SEARCH_LIMIT_MAX}"),
            )));
        }
    }
    let identity = resolve_identity(&state.workspace)?;
    let input = SearchInput {
        query: q.q,
        mode: q.mode.unwrap_or_default(),
        trust: q.trust,
        kinds: q.kind.into_iter().collect(),
        scope: q.scope,
        limit: q.limit.unwrap_or(20),
        include_quarantine: q.include_quarantine,
        request_id: request_id(&headers),
    };
    // Synchronous, potentially multi-second op (embedding / embed-daemon
    // spawn): run on the blocking pool so semantic searches never starve the
    // async runtime (firetrail-paag).
    let out = tokio::task::spawn_blocking(move || {
        memory::search::search(&state.workspace, &identity, input, &state.events)
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("search task failed: {e}")))??;
    Ok((StatusCode::OK, Json(out)))
}

/// Query params for `GET /api/memory/similar/:id`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarQuery {
    /// Cap hits (1..=100). Defaults to 10.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// `GET /api/memory/similar/:id` — nearest-neighbour records.
#[tracing::instrument(skip_all)]
pub async fn similar_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<SimilarQuery>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(n) = q.limit {
        if n == 0 || n > SEARCH_LIMIT_MAX {
            return Err(AppError::Ops(ft_ops::OpsError::validation(
                "limit",
                format!("must be in 1..={SEARCH_LIMIT_MAX}"),
            )));
        }
    }
    let identity = resolve_identity(&state.workspace)?;
    let input = SimilarInput {
        id,
        limit: q.limit.unwrap_or(10),
        request_id: request_id(&headers),
    };
    // `similar` embeds the seed record, so it shares search's blocking profile;
    // keep it off the async workers too (firetrail-paag).
    let out = tokio::task::spawn_blocking(move || {
        memory::search::similar(&state.workspace, &identity, input, &state.events)
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("similar task failed: {e}")))??;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Salvage.
// ─────────────────────────────────────────────────────────────────────────────

/// `POST /api/memory/salvage` — dry-run-then-apply salvage workflow.
#[tracing::instrument(skip_all)]
pub async fn salvage_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<SalvageInput>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let mut input = body.map_or_else(
        || SalvageInput {
            base: "main".to_string(),
            branch: None,
            dry_run: false,
            selected: None,
            request_id: None,
        },
        |Json(b)| b,
    );
    input.request_id = input.request_id.or_else(|| request_id(&headers));
    let out = memory::salvage::salvage(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}
