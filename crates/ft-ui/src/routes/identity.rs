//! `/api/identity` HTTP surface — Wave 3 Batch B of the firetrail GUI.
//!
//! Route table:
//!
//! | Method | Path                                  | Op                                       |
//! |--------|---------------------------------------|------------------------------------------|
//! | GET    | `/api/identity`                       | [`ft_ops::identity_ops::list`]           |
//! | POST   | `/api/identity`                       | [`ft_ops::identity_ops::register`]       |
//! | GET    | `/api/identity/:id`                   | [`ft_ops::identity_ops::show`]           |
//! | POST   | `/api/identity/:id/offboard`          | [`ft_ops::identity_ops::offboard`]       |
//! | GET    | `/api/identity/:id/capabilities`      | [`ft_ops::identity_ops::capabilities`]   |
//! | PATCH  | `/api/identity/:id/capabilities`      | [`ft_ops::identity_ops::update_capabilities`] |
//!
//! Writes thread `X-Firetrail-Request-Id` through to the
//! [`ft_ops::Event::IdentityUpdated`] envelope so optimistic GUI updates
//! can coalesce. Reads ignore the header.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::identity_ops::{
    self, CapabilitiesInput as IdentityCapabilitiesInput, CapabilityOverrideInput, CapabilityPatch,
    IdentityKindInput, IdentityStatusFilter, ListInput as IdentityListInput,
    OffboardInput as IdentityOffboardInput, RegisterInput as IdentityRegisterInput,
    ShowInput as IdentityShowInput, UpdateCapabilitiesInput as IdentityUpdateCapabilitiesInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

use super::tickets::REQUEST_ID_HEADER;

/// Build the `/api/identity` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_handler).post(register_handler))
        .route("/:id", get(show_handler))
        .route("/:id/offboard", post(offboard_handler))
        .route(
            "/:id/capabilities",
            get(capabilities_handler).patch(update_capabilities_handler),
        )
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

/// Query for `GET /api/identity`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// Filter by lifecycle status.
    #[serde(default)]
    pub status: Option<IdentityStatusFilter>,
    /// Filter by identity kind (`human` / `bot` / `ci`). Applied
    /// client-side over the registry view; not pushed to the ops layer
    /// because the ops `list` op doesn't yet accept a kind filter (the
    /// registry is small enough that filtering at the route boundary
    /// keeps the ops surface stable).
    #[serde(default)]
    pub kind: Option<IdentityKindInput>,
}

/// `GET /api/identity` — listing with optional status / kind filters.
#[tracing::instrument(skip_all)]
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = identity_ops::list(
        &state.workspace,
        &identity,
        IdentityListInput {
            status: q.status,
            request_id: None,
        },
        &state.events,
    )?;
    let kind_filter = q.kind.map(|k| match k {
        IdentityKindInput::Human => "human",
        IdentityKindInput::Bot => "bot",
        IdentityKindInput::Ci => "ci",
    });
    let filtered = if let Some(want) = kind_filter {
        ft_ops::identity_ops::IdentityListOutput {
            identities: out
                .identities
                .into_iter()
                .filter(|i| i.kind == want)
                .collect(),
        }
    } else {
        out
    };
    Ok((StatusCode::OK, Json(filtered)))
}

/// `GET /api/identity/:id` — single registered identity.
#[tracing::instrument(skip_all)]
pub async fn show_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = identity_ops::show(
        &state.workspace,
        &identity,
        IdentityShowInput {
            id,
            request_id: None,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `POST /api/identity`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    /// Canonical short id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Email aliases.
    pub emails: Vec<String>,
    /// Identity kind.
    pub kind: IdentityKindInput,
    /// Machine hostnames.
    #[serde(default)]
    pub machines: Vec<String>,
    /// Capability overrides.
    #[serde(default)]
    pub capabilities: Vec<CapabilityOverrideInput>,
}

/// `POST /api/identity` — register a new identity.
#[tracing::instrument(skip_all)]
pub async fn register_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = IdentityRegisterInput {
        id: body.id,
        name: body.name,
        emails: body.emails,
        kind: body.kind,
        machines: body.machines,
        capabilities: body.capabilities,
        request_id: request_id(&headers),
    };
    let out = identity_ops::register(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::CREATED, Json(out)))
}

/// Body for `POST /api/identity/:id/offboard`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffboardBody {
    /// Walk every record and release the identity's live claims.
    #[serde(default)]
    pub sweep_claims: bool,
}

/// `POST /api/identity/:id/offboard` — flip status to offboarded.
#[tracing::instrument(skip_all)]
pub async fn offboard_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<OffboardBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = IdentityOffboardInput {
        id,
        sweep_claims: body.sweep_claims,
        request_id: request_id(&headers),
    };
    let out = identity_ops::offboard(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// Body for `PATCH /api/identity/:id/capabilities`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapabilitiesBody {
    /// Patches to apply. Each `value` is tri-state: `null` clears the
    /// override; `true`/`false` set it.
    pub capabilities: Vec<CapabilityPatch>,
}

/// `PATCH /api/identity/:id/capabilities` — update capability overrides.
#[tracing::instrument(skip_all)]
pub async fn update_capabilities_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateCapabilitiesBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = IdentityUpdateCapabilitiesInput {
        id,
        capabilities: body.capabilities,
        request_id: request_id(&headers),
    };
    let out = identity_ops::update_capabilities(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/identity/:id/capabilities` — effective capability matrix.
#[tracing::instrument(skip_all)]
pub async fn capabilities_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = identity_ops::capabilities(
        &state.workspace,
        &identity,
        IdentityCapabilitiesInput {
            id,
            request_id: None,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}
