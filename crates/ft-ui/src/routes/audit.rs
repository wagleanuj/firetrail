//! `/api/audit` HTTP surface — Wave 3 Batch B of the firetrail GUI.
//!
//! Route table:
//!
//! | Method | Path                                                              | Op                                       |
//! |--------|-------------------------------------------------------------------|------------------------------------------|
//! | POST   | `/api/audit/lint`                                                 | [`ft_ops::audit::lint`]                  |
//! | POST   | `/api/audit/verify`                                               | [`ft_ops::audit::verify`]                |
//! | GET    | `/api/audit/review/:id`                                           | [`ft_ops::audit::review`]                |
//! | GET    | `/api/audit/criteria/:id`                                         | [`ft_ops::audit::criteria_list`]         |
//! | POST   | `/api/audit/criteria/:id`                                         | [`ft_ops::audit::criteria_add`]          |
//! | PATCH  | `/api/audit/criteria/:id/:which`                                  | [`ft_ops::audit::criteria_check`] / `_uncheck` |
//! | POST   | `/api/audit/criteria/:id/:which/evidence`                         | [`ft_ops::audit::criteria_evidence`]     |
//! | GET    | `/api/audit/diff`                                                 | [`ft_ops::audit::diff`]                  |
//! | GET    | `/api/audit/graph`                                                | [`ft_ops::audit::graph`]                 |
//!
//! ## `lint` and `verify` as POST
//!
//! Both ops are technically read-only over the on-disk state, but they
//! emit [`ft_ops::Event::LintRun`] / [`ft_ops::Event::VerifyRun`] events
//! and can take a non-trivial amount of time on large workspaces. We
//! model them as `POST` ("run-now") rather than `GET` so:
//!
//! 1. The `X-Firetrail-Request-Id` thread-through actually correlates with
//!    the SSE event a subscriber receives.
//! 2. Clients do not accidentally GET-cache the result.
//!
//! See firetrail-h4u for a P3 follow-up to make these async-with-progress
//! once the wall-time exceeds a few seconds on realistic fixtures.
//!
//! ## `PATCH /api/audit/criteria/:id/:which`
//!
//! Body: `{ "checked": true|false }`. Maps onto
//! [`ft_ops::audit::criteria_check`] or `_uncheck`. `:which` accepts either
//! an `ac-NN` id or a 1-based index (the ops layer parses both).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, patch, post},
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::audit::{
    self, CriteriaAddInput, CriteriaEvidenceInput, CriteriaListInput, CriteriaToggleInput,
    DiffInput, GraphDirectionInput, GraphInput, LintInput, ReviewInput as AuditReviewInput,
    VerifyInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

use super::tickets::REQUEST_ID_HEADER;

/// Hard cap on the `depth` query param for `/api/audit/graph`.
const GRAPH_DEPTH_MAX: u32 = 5;

/// Build the `/api/audit` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/lint", post(lint_handler))
        .route("/verify", post(verify_handler))
        .route("/review/:id", get(review_handler))
        .route(
            "/criteria/:id",
            get(criteria_list_handler).post(criteria_add_handler),
        )
        .route("/criteria/:id/:which", patch(criteria_toggle_handler))
        .route(
            "/criteria/:id/:which/evidence",
            post(criteria_evidence_handler),
        )
        .route("/diff", get(diff_handler))
        .route("/graph", get(graph_handler))
}

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
// Lint / verify
// ─────────────────────────────────────────────────────────────────────────────

/// Body for `POST /api/audit/lint`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintBody {
    /// Emit suggested-fix hints on every finding.
    #[serde(default)]
    pub fix_hints: bool,
}

/// `POST /api/audit/lint`.
#[tracing::instrument(skip_all)]
pub async fn lint_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<LintBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = LintInput {
        fix_hints: body.fix_hints,
        request_id: request_id(&headers),
    };
    let out = audit::lint(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/audit/verify`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyBody {
    /// Optional specific record id; otherwise walks every record.
    #[serde(default)]
    pub id: Option<String>,
}

/// `POST /api/audit/verify`.
#[tracing::instrument(skip_all)]
pub async fn verify_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<VerifyBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = VerifyInput {
        id: body.id,
        request_id: request_id(&headers),
    };
    let out = audit::verify(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Review (read-only summary)
// ─────────────────────────────────────────────────────────────────────────────

/// `GET /api/audit/review/:id`.
#[tracing::instrument(skip_all)]
pub async fn review_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = audit::review(
        &state.workspace,
        &identity,
        AuditReviewInput {
            id,
            request_id: None,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Criteria CRUD
// ─────────────────────────────────────────────────────────────────────────────

/// `GET /api/audit/criteria/:id`.
#[tracing::instrument(skip_all)]
pub async fn criteria_list_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = audit::criteria_list(
        &state.workspace,
        &identity,
        CriteriaListInput {
            id,
            request_id: None,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/audit/criteria/:id`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaAddBody {
    /// Criterion text.
    pub text: String,
}

/// `POST /api/audit/criteria/:id`.
#[tracing::instrument(skip_all)]
pub async fn criteria_add_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CriteriaAddBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = CriteriaAddInput {
        id,
        text: body.text,
        request_id: request_id(&headers),
    };
    let out = audit::criteria_add(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::CREATED, Json(out)))
}

/// Body for `PATCH /api/audit/criteria/:id/:which`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaTogglePatch {
    /// Target checked state.
    pub checked: bool,
}

/// `PATCH /api/audit/criteria/:id/:which`.
#[tracing::instrument(skip_all)]
pub async fn criteria_toggle_handler(
    State(state): State<Arc<AppState>>,
    Path((id, which)): Path<(String, String)>,
    headers: HeaderMap,
    Json(patch): Json<CriteriaTogglePatch>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = CriteriaToggleInput {
        id,
        which,
        request_id: request_id(&headers),
    };
    let out = if patch.checked {
        audit::criteria_check(&state.workspace, &identity, input, &state.events)?
    } else {
        audit::criteria_uncheck(&state.workspace, &identity, input, &state.events)?
    };
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/audit/criteria/:id/:which/evidence`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaEvidenceBody {
    /// Evidence URL.
    pub url: String,
}

/// `POST /api/audit/criteria/:id/:which/evidence`.
#[tracing::instrument(skip_all)]
pub async fn criteria_evidence_handler(
    State(state): State<Arc<AppState>>,
    Path((id, which)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<CriteriaEvidenceBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = CriteriaEvidenceInput {
        id,
        which,
        url: body.url,
        request_id: request_id(&headers),
    };
    let out = audit::criteria_evidence(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Diff
// ─────────────────────────────────────────────────────────────────────────────

/// Query for `GET /api/audit/diff`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffQuery {
    /// Base git ref.
    pub base: String,
    /// Head git ref.
    pub head: String,
    /// Restrict to memory-kind changes.
    #[serde(default)]
    pub memory_only: bool,
    /// Scope prefix filter.
    #[serde(default)]
    pub scope: Option<String>,
}

/// `GET /api/audit/diff`.
#[tracing::instrument(skip_all)]
pub async fn diff_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<DiffQuery>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = DiffInput {
        base: q.base,
        head: q.head,
        memory_only: q.memory_only,
        scope: q.scope,
        request_id: None,
    };
    let out = audit::diff(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Graph
// ─────────────────────────────────────────────────────────────────────────────

/// Query for `GET /api/audit/graph`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQuery {
    /// Root record id (full canonical or unambiguous prefix).
    pub id: String,
    /// Walk direction (`up | down | both`); defaults to `both`.
    #[serde(default)]
    pub direction: Option<GraphDirectionInput>,
    /// Walk depth (1..=5).
    #[serde(default)]
    pub depth: Option<u32>,
}

/// `GET /api/audit/graph`.
#[tracing::instrument(skip_all)]
pub async fn graph_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GraphQuery>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(d) = q.depth {
        if d == 0 || d > GRAPH_DEPTH_MAX {
            return Err(AppError::Ops(ft_ops::OpsError::validation(
                "depth",
                format!("must be in 1..={GRAPH_DEPTH_MAX}"),
            )));
        }
    }
    let identity = resolve_identity(&state.workspace)?;
    let input = GraphInput {
        id: q.id,
        direction: q.direction.unwrap_or(GraphDirectionInput::Both),
        depth: q.depth.unwrap_or(2),
        request_id: None,
    };
    let out = audit::graph(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}
