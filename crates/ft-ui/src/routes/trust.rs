//! `/api/trust` HTTP surface — Wave 3 Batch B of the firetrail GUI.
//!
//! Route table (every endpoint is a memory-record trust transition):
//!
//! | Method | Path                                | Op                                  | Returns                  |
//! |--------|-------------------------------------|-------------------------------------|--------------------------|
//! | POST   | `/api/trust/:id/review`             | [`ft_ops::trust::review`]           | `TrustOutput`            |
//! | POST   | `/api/trust/:id/promote`            | [`ft_ops::trust::promote`]          | `TrustOutput`            |
//! | POST   | `/api/trust/:id/deprecate`          | [`ft_ops::trust::deprecate`]        | `TrustOutput`            |
//! | POST   | `/api/trust/:id/archive`            | [`ft_ops::trust::archive`]          | `TrustOutput`            |
//! | POST   | `/api/trust/:id/supersede`          | [`ft_ops::trust::supersede`]        | `TrustOutput`            |
//! | POST   | `/api/trust/:id/redact`             | [`ft_ops::trust::redact`]           | `TrustOutput`            |
//! | POST   | `/api/trust/:id/merge`              | [`ft_ops::trust::merge`]            | `MergeOutput`            |
//!
//! `:id` on `merge` is the **canonical** record id (every entry in
//! `sources` is superseded by `:id`). The body shape varies per route —
//! see each handler doc-comment.
//!
//! ## State machine
//!
//! The ops layer enforces ADR-0013 via `ft_trust::validate_transition`. If
//! a record is already in the target state (e.g. you POST `/promote` on a
//! `verified` record), the ops layer returns
//! [`ft_ops::OpsError::Conflict`] which renders as **HTTP 409**. Validation
//! failures (missing evidence on a high-risk promote, illegal transition)
//! return **HTTP 400**.
//!
//! ## Request id thread-through
//!
//! Every handler reads `X-Firetrail-Request-Id` and copies it onto the
//! corresponding ops input. The matching [`ft_ops::Event::TrustTransitioned`]
//! envelope carries the same id back to subscribers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::trust::{
    self, ArchiveInput, EvidenceKindInput, MergeInput, PromoteInput, ReasonInput,
    ReviewInput as TrustReviewInput, SupersedeInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

use super::tickets::REQUEST_ID_HEADER;

/// Build the `/api/trust` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/:id/review", post(review_handler))
        .route("/:id/promote", post(promote_handler))
        .route("/:id/deprecate", post(deprecate_handler))
        .route("/:id/archive", post(archive_handler))
        .route("/:id/supersede", post(supersede_handler))
        .route("/:id/redact", post(redact_handler))
        .route("/:id/merge", post(merge_handler))
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

/// Body for `POST /api/trust/:id/review`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewBody {
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional evidence URL.
    #[serde(default)]
    pub evidence_url: Option<String>,
}

/// `POST /api/trust/:id/review` — Draft → Reviewed.
#[tracing::instrument(skip_all)]
pub async fn review_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<ReviewBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = TrustReviewInput {
        id,
        reason: body.reason,
        evidence_url: body.evidence_url,
        request_id: request_id(&headers),
    };
    let out = trust::review(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/trust/:id/promote`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteBody {
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
    /// Evidence URL — required for high-risk classes per ADR-0013.
    #[serde(default)]
    pub evidence_url: Option<String>,
    /// Evidence kind.
    #[serde(default)]
    pub evidence_type: Option<EvidenceKindInput>,
}

/// `POST /api/trust/:id/promote` — Reviewed → Verified.
#[tracing::instrument(skip_all)]
pub async fn promote_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<PromoteBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = PromoteInput {
        id,
        reason: body.reason,
        evidence_url: body.evidence_url,
        evidence_type: body.evidence_type,
        request_id: request_id(&headers),
    };
    let out = trust::promote(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/trust/:id/deprecate` and `/redact`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonBody {
    /// Required reason text.
    pub reason: String,
}

/// `POST /api/trust/:id/deprecate`.
#[tracing::instrument(skip_all)]
pub async fn deprecate_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReasonBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = ReasonInput {
        id,
        reason: body.reason,
        request_id: request_id(&headers),
    };
    let out = trust::deprecate(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `POST /api/trust/:id/archive`.
#[tracing::instrument(skip_all)]
pub async fn archive_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = ArchiveInput {
        id,
        request_id: request_id(&headers),
    };
    let out = trust::archive(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/trust/:id/supersede`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupersedeBody {
    /// Successor memory id.
    pub successor: String,
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/trust/:id/supersede`.
#[tracing::instrument(skip_all)]
pub async fn supersede_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SupersedeBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = SupersedeInput {
        id,
        successor: body.successor,
        reason: body.reason,
        request_id: request_id(&headers),
    };
    let out = trust::supersede(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `POST /api/trust/:id/redact`.
#[tracing::instrument(skip_all)]
pub async fn redact_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReasonBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = ReasonInput {
        id,
        reason: body.reason,
        request_id: request_id(&headers),
    };
    let out = trust::redact(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/trust/:id/merge`.
///
/// `:id` is the canonical id; every entry in `sources` is superseded by it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeBody {
    /// Source ids to merge into `:id`.
    #[serde(alias = "others")]
    pub sources: Vec<String>,
    /// Optional reason text (default applied per-record by ops).
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/trust/:id/merge`.
#[tracing::instrument(skip_all)]
pub async fn merge_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<MergeBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = MergeInput {
        canonical: id,
        others: body.sources,
        reason: body.reason,
        request_id: request_id(&headers),
    };
    let out = trust::merge(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}
