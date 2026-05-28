//! `/api/tickets` HTTP surface — Wave 1 Batch B of the firetrail GUI.
//!
//! Route table:
//!
//! | Method | Path                                | Op                             |
//! |--------|-------------------------------------|--------------------------------|
//! | GET    | `/api/tickets`                      | [`ft_ops::tickets::list`]      |
//! | GET    | `/api/tickets/board`                | [`ft_ops::tickets::board`]     |
//! | GET    | `/api/tickets/:id`                  | [`ft_ops::tickets::show`]      |
//! | POST   | `/api/tickets`                      | discriminated create           |
//! | PATCH  | `/api/tickets/:id`                  | [`ft_ops::tickets::update`]    |
//! | POST   | `/api/tickets/:id/claim`            | [`ft_ops::tickets::claim`]     |
//! | POST   | `/api/tickets/:id/unclaim`          | [`ft_ops::tickets::unclaim`]   |
//! | POST   | `/api/tickets/:id/close`            | [`ft_ops::tickets::close`]     |
//! | POST   | `/api/tickets/:id/links`            | [`ft_ops::tickets::link`]      |
//!
//! ## Discriminated create
//!
//! `POST /api/tickets` accepts a single JSON body tagged with `kind`:
//!
//! ```json
//! { "kind": "task", "title": "…", "epic": "…", "owner": "…", … }
//! ```
//!
//! Variants: `"epic"`, `"task"`, `"subtask"`, `"bug"`. This keeps the
//! POST URL stable for the SPA (mirrors REST "POST to collection creates
//! a member") and lets ts-rs surface each input shape verbatim.
//!
//! ## Request id coalescing
//!
//! Every write accepts an optional `X-Firetrail-Request-Id` header. The
//! handler copies it onto the corresponding ops input's `request_id`
//! field; ops then emit through `EventBus::emit_with_request` so the
//! same client can filter its own SSE echoes.
//!
//! ## Identity
//!
//! For Wave 1 the loopback server runs as the logged-in OS user. We
//! resolve the actor via `ft_identity::DefaultResolver` (the same path
//! the CLI uses — see `crates/ft-cli/src/commands/claim.rs` and
//! `crates/ft-cli/src/context.rs`).
//!
//! TODO: real per-user auth is a future wave; the bootstrap-token +
//! session-cookie pair is the only thing standing between an attacker
//! and these routes today (plus the `Host`/`Origin` checks). When the
//! GUI grows to multi-user we will switch this to an identity attached
//! to the session at bootstrap time.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::tickets::{
    self, BoardInput, ClaimInput, CloseInput, CreateBugInput, CreateEpicInput,
    CreateSubtaskInput, CreateTaskInput, LinkInput, ListInput, ShowInput, TicketRelationKind,
    UnclaimInput, UpdateInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

/// HTTP header carrying the optional client correlation id.
pub const REQUEST_ID_HEADER: &str = "x-firetrail-request-id";

/// Build the `/api/tickets` sub-router.
///
/// Mounted under `/api/tickets` by [`crate::routes::build`]. The session
/// middleware applied by the parent router still guards every route.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_handler).post(create_handler))
        .route("/board", get(board_handler))
        .route("/:id", get(show_handler).patch(update_handler))
        .route("/:id/claim", post(claim_handler))
        .route("/:id/unclaim", post(unclaim_handler))
        .route("/:id/close", post(close_handler))
        .route("/:id/links", post(link_handler))
}

// ─────────────────────────────────────────────────────────────────────────────
// Identity + request-id extraction.
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the request's actor from the ambient environment.
///
/// V1 falls back to the same resolver chain ft-cli uses
/// (`FIRETRAIL_AUTHOR` → workspace config → git config). Returns a
/// permission-denied error if the resolver cannot produce an identity,
/// mirroring CLI behaviour.
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

/// Extract the optional `X-Firetrail-Request-Id` header value.
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

/// `GET /api/tickets` — index-backed ticket query.
#[tracing::instrument(skip_all)]
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
    Query(input): Query<ListInput>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = tickets::list(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/tickets/board` — kanban snapshot grouped by status.
#[tracing::instrument(skip_all)]
pub async fn board_handler(
    State(state): State<Arc<AppState>>,
    Query(input): Query<BoardInput>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = tickets::board(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/tickets/:id` — a single ticket plus its relations.
#[tracing::instrument(skip_all)]
pub async fn show_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = tickets::show(&state.workspace, &identity, ShowInput { id }, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Create.
// ─────────────────────────────────────────────────────────────────────────────

/// Discriminated create body for `POST /api/tickets`.
///
/// The `kind` field selects which `ft_ops::tickets::create_*` op runs.
/// Each variant carries the create input verbatim (minus its own
/// `request_id`, which we fill from the header).
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CreateBody {
    /// Create an epic.
    Epic(CreateEpicInput),
    /// Create a task.
    Task(CreateTaskInput),
    /// Create a subtask.
    Subtask(CreateSubtaskInput),
    /// Create a bug.
    Bug(CreateBugInput),
}

/// `POST /api/tickets` — discriminated create.
#[tracing::instrument(skip_all)]
pub async fn create_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let rid = request_id(&headers);
    let out = match body {
        CreateBody::Epic(mut input) => {
            input.request_id = input.request_id.or(rid);
            tickets::create_epic(&state.workspace, &identity, input, &state.events)?
        }
        CreateBody::Task(mut input) => {
            input.request_id = input.request_id.or(rid);
            tickets::create_task(&state.workspace, &identity, input, &state.events)?
        }
        CreateBody::Subtask(mut input) => {
            input.request_id = input.request_id.or(rid);
            tickets::create_subtask(&state.workspace, &identity, input, &state.events)?
        }
        CreateBody::Bug(mut input) => {
            input.request_id = input.request_id.or(rid);
            tickets::create_bug(&state.workspace, &identity, input, &state.events)?
        }
    };
    Ok((StatusCode::CREATED, Json(out)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Update / claim / unclaim / close / link.
// ─────────────────────────────────────────────────────────────────────────────

/// Patch body for `PATCH /api/tickets/:id`.
///
/// Mirrors [`UpdateInput`] minus the id (which comes from the path) and
/// the `request_id` (which comes from the header). An empty payload is
/// rejected with `400` to match CLI semantics.
#[derive(Debug, Default, Deserialize)]
pub struct UpdatePatch {
    /// New title.
    #[serde(default)]
    pub title: Option<String>,
    /// New status.
    #[serde(default)]
    pub status: Option<ft_ops::tickets::TicketStatusFilter>,
    /// New priority.
    #[serde(default)]
    pub priority: Option<ft_ops::tickets::TicketPriority>,
    /// New owner (empty string clears).
    #[serde(default)]
    pub owner: Option<String>,
    /// New description.
    #[serde(default)]
    pub description: Option<String>,
}

impl UpdatePatch {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.status.is_none()
            && self.priority.is_none()
            && self.owner.is_none()
            && self.description.is_none()
    }
}

/// `PATCH /api/tickets/:id` — partial envelope update.
#[tracing::instrument(skip_all)]
pub async fn update_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(patch): Json<UpdatePatch>,
) -> Result<impl IntoResponse, AppError> {
    if patch.is_empty() {
        return Err(AppError::Ops(ft_ops::OpsError::validation(
            "input",
            "empty patch; supply at least one of title, status, priority, owner, description",
        )));
    }
    let identity = resolve_identity(&state.workspace)?;
    let input = UpdateInput {
        id,
        title: patch.title,
        status: patch.status,
        priority: patch.priority,
        owner: patch.owner,
        description: patch.description,
        request_id: request_id(&headers),
    };
    let out = tickets::update(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `POST /api/tickets/:id/claim`.
#[derive(Debug, Default, Deserialize)]
pub struct ClaimBody {
    /// Optional human-readable duration (e.g. `"7d"`, `"12h"`).
    #[serde(default)]
    pub expires: Option<String>,
}

/// `POST /api/tickets/:id/claim` — atomically claim a ticket.
#[tracing::instrument(skip_all)]
pub async fn claim_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<ClaimBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = ClaimInput {
        id,
        expires: body.expires,
        request_id: request_id(&headers),
    };
    let out = tickets::claim(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `POST /api/tickets/:id/unclaim`.
#[derive(Debug, Default, Deserialize)]
pub struct UnclaimBody {
    /// Release another actor's claim.
    #[serde(default)]
    pub takeover: bool,
    /// Required when `takeover` is `true`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/tickets/:id/unclaim` — release a claim.
#[tracing::instrument(skip_all)]
pub async fn unclaim_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<UnclaimBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = UnclaimInput {
        id,
        takeover: body.takeover,
        reason: body.reason,
        request_id: request_id(&headers),
    };
    let out = tickets::unclaim(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `POST /api/tickets/:id/close`.
#[derive(Debug, Default, Deserialize)]
pub struct CloseBody {
    /// Skip acceptance-criteria validation. Requires `reason`.
    #[serde(default)]
    pub force: bool,
    /// Reason; required when `force` is `true`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/tickets/:id/close` — transition to `Closed`.
#[tracing::instrument(skip_all)]
pub async fn close_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<CloseBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = CloseInput {
        id,
        force: body.force,
        reason: body.reason,
        request_id: request_id(&headers),
    };
    let out = tickets::close(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `POST /api/tickets/:id/links`.
#[derive(Debug, Deserialize)]
pub struct LinkBody {
    /// Target ticket id.
    pub to: String,
    /// Relation kind.
    pub kind: TicketRelationKind,
}

/// `POST /api/tickets/:id/links` — append a relation edge.
#[tracing::instrument(skip_all)]
pub async fn link_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<LinkBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = LinkInput {
        from: id,
        to: body.to,
        kind: body.kind,
        request_id: request_id(&headers),
    };
    let out = tickets::link(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}
