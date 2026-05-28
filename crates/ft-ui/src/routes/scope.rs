//! `/api/scope` HTTP surface — Wave 3 Batch B of the firetrail GUI.
//!
//! Route table:
//!
//! | Method | Path                          | Op                                |
//! |--------|-------------------------------|-----------------------------------|
//! | GET    | `/api/scope`                  | [`ft_ops::scope::list`]           |
//! | GET    | `/api/scope/aliases`          | [`ft_ops::scope::aliases`]        |
//! | GET    | `/api/scope/owners`           | [`ft_ops::scope::owners`]         |
//! | GET    | `/api/scope/:id`              | [`ft_ops::scope::show`]           |
//!
//! All routes are read-only and ignore the `X-Firetrail-Request-Id` header.
//! `aliases` and `owners` are intentionally **flat** sub-paths (not nested
//! under `:id`) because the registry surfaces them globally — there is no
//! per-scope owners endpoint at the ops layer (`owners_for_path` takes a
//! file path, not a scope id).

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_ops::scope::{
    self, AliasesInput as ScopeAliasesInput, ListInput as ScopeListInput,
    OwnersInput as ScopeOwnersInput, ShowInput as ScopeShowInput,
};
use ft_ops::{Identity, Workspace};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

/// Build the `/api/scope` sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_handler))
        .route("/aliases", get(aliases_handler))
        .route("/owners", get(owners_handler))
        // `/:id` is the catch-all; keep it last so the named routes above win.
        .route("/:id", get(show_handler))
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
