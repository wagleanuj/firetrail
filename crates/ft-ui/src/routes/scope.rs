//! `/api/scope` HTTP surface — Wave 3 Batch B + scope-authoring writes.
//!
//! Route table:
//!
//! | Method | Path                          | Op                                |
//! |--------|-------------------------------|-----------------------------------|
//! | GET    | `/api/scope`                  | [`ft_ops::scope::list`]           |
//! | POST   | `/api/scope`                  | [`ft_ops::scope::add`]            |
//! | GET    | `/api/scope/aliases`          | [`ft_ops::scope::aliases`]        |
//! | GET    | `/api/scope/owners`           | [`ft_ops::scope::owners`]         |
//! | GET    | `/api/scope/preview`          | [`ft_ops::scope::preview`]        |
//! | POST   | `/api/scope/reorder`          | [`ft_ops::scope::reorder`]        |
//! | PUT    | `/api/scope/:id`              | [`ft_ops::scope::edit`]           |
//! | DELETE | `/api/scope/:id`              | [`ft_ops::scope::remove`]         |
//! | GET    | `/api/scope/:id`              | [`ft_ops::scope::show`]           |
//!
//! The read routes ignore the `X-Firetrail-Request-Id` header; each write
//! thread it onto the ops input so the matching [`ft_ops::Event::ScopeUpdated`]
//! envelope carries it back to SSE subscribers. `aliases` and `owners` are
//! intentionally **flat** sub-paths (not nested under `:id`) because the
//! registry surfaces them globally — there is no per-scope owners endpoint at
//! the ops layer (`owners_for_path` takes a file path, not a scope id).

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::scope::{
    self, AliasesInput as ScopeAliasesInput, ListInput as ScopeListInput,
    OwnersInput as ScopeOwnersInput, ScopeEditInput, ScopeInput, ShowInput as ScopeShowInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

use super::tickets::REQUEST_ID_HEADER;

/// Build the `/api/scope` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_handler).post(add_handler))
        .route("/aliases", get(aliases_handler))
        .route("/owners", get(owners_handler))
        .route("/preview", get(preview_handler))
        .route("/reorder", post(reorder_handler))
        // `/:id` is the catch-all; keep it last so the named routes above win.
        .route(
            "/:id",
            get(show_handler).put(edit_handler).delete(remove_handler),
        )
}

/// Pull the optional `X-Firetrail-Request-Id` correlation id off the headers.
fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
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

/// `GET /api/scope` — every scope registered in `.firetrail/scopes.yaml`.
#[tracing::instrument(skip_all)]
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::list(
        &state.workspace,
        &identity,
        ScopeListInput::default(),
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/scope/aliases` — flat alphabetical alias → scope-id map.
#[tracing::instrument(skip_all)]
pub async fn aliases_handler(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::aliases(
        &state.workspace,
        &identity,
        ScopeAliasesInput::default(),
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// Query for `GET /api/scope/owners`.
#[derive(Debug, Deserialize)]
pub struct OwnersQuery {
    /// Repo-relative or absolute path to look up.
    pub path: PathBuf,
}

/// `GET /api/scope/owners?path=…` — resolve CODEOWNERS for a path.
#[tracing::instrument(skip_all)]
pub async fn owners_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<OwnersQuery>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::owners(
        &state.workspace,
        &identity,
        ScopeOwnersInput {
            path: q.path,
            request_id: None,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/scope/:id` — detail view (summary + CODEOWNERS rows).
#[tracing::instrument(skip_all)]
pub async fn show_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::show(
        &state.workspace,
        &identity,
        ScopeShowInput {
            id,
            request_id: None,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `POST /api/scope` — a complete new scope.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddScopeBody {
    /// Canonical scope id (unique).
    pub id: String,
    /// Optional display name.
    #[serde(default)]
    pub name: Option<String>,
    /// `applies_to` glob patterns (at least one required).
    #[serde(default)]
    pub applies_to: Vec<String>,
    /// Declared aliases.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Optional CODEOWNERS path (repo-relative).
    #[serde(default)]
    pub codeowners: Option<PathBuf>,
}

/// `POST /api/scope` — append a new scope (becomes last-declared). 409 on a
/// duplicate id, 400 on an invalid glob / empty `applies_to`.
#[tracing::instrument(skip_all)]
pub async fn add_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AddScopeBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::add(
        &state.workspace,
        &identity,
        ScopeInput {
            id: body.id,
            name: body.name,
            applies_to: body.applies_to,
            aliases: body.aliases,
            codeowners: body.codeowners,
            request_id: request_id(&headers),
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `PUT /api/scope/:id` — a partial edit.
///
/// `Option<Option<T>>` on the nullable fields mirrors the ops input: a missing
/// key leaves the field untouched; `null` clears it; a value sets it. Vec
/// fields replace the stored list when present.
#[allow(clippy::option_option)]
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditScopeBody {
    /// New display name (`null` clears it).
    #[serde(default)]
    pub name: Option<Option<String>>,
    /// Replacement `applies_to` glob list.
    #[serde(default)]
    pub applies_to: Option<Vec<String>>,
    /// Replacement alias list.
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    /// New CODEOWNERS path (`null` clears it).
    #[serde(default)]
    pub codeowners: Option<Option<PathBuf>>,
}

/// `PUT /api/scope/:id` — apply only the provided changes. 404 when absent,
/// 400 on an invalid glob / empty `applies_to`.
#[tracing::instrument(skip_all)]
pub async fn edit_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<EditScopeBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let out = scope::edit(
        &state.workspace,
        &identity,
        &id,
        ScopeEditInput {
            name: body.name,
            applies_to: body.applies_to,
            aliases: body.aliases,
            codeowners: body.codeowners,
            request_id: request_id(&headers),
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}

/// `DELETE /api/scope/:id` — remove a scope by id. 404 when absent.
#[tracing::instrument(skip_all)]
pub async fn remove_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::remove(&state.workspace, &identity, &id, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// JSON body for `POST /api/scope/reorder` — the full ordered id list.
#[derive(Debug, Deserialize)]
pub struct ReorderBody {
    /// The complete set of existing scope ids, in the desired order.
    pub ids: Vec<String>,
}

/// `POST /api/scope/reorder` — reorder scopes to the given full id list.
/// 400 when `ids` is not a permutation of the existing scope ids.
#[tracing::instrument(skip_all)]
pub async fn reorder_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ReorderBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::reorder(&state.workspace, &identity, &body.ids, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}

/// `GET /api/scope/preview` — per-scope tracked-file match counts plus coverage
/// warnings (zero-match globs, broad-last shadowing) for the live editor.
#[tracing::instrument(skip_all)]
pub async fn preview_handler(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = scope::preview(&state.workspace, &identity, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}
