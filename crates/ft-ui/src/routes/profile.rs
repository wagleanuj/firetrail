//! `/api/profile` HTTP surface — the `RepoProfile` read/edit panel.
//!
//! | Method | Path                              | Op                                  | Returns           |
//! |--------|-----------------------------------|-------------------------------------|-------------------|
//! | GET    | `/api/profile[?scope=&resolved=]` | [`ft_ops::profile::get`] / [`ft_ops::profile::get_for_scope`] | `ProfileView`     |
//! | PUT    | `/api/profile[?scope=]`           | [`ft_ops::profile::update`] / [`ft_ops::profile::update_for_scope`] | `ProfileView`     |
//! | GET    | `/api/profile/resolve?paths=`     | [`ft_ops::profile::validate_plan`]  | `ValidatePlanView`|
//! | POST   | `/api/profile/components`         | [`ft_ops::profile::add_component`]  | `ProfileView`     |
//! | DELETE | `/api/profile/components/:name`   | [`ft_ops::profile::remove_component`] | `ProfileView`   |
//!
//! `?scope=<id>` selects the per-scope delta record; `&resolved=1` returns the
//! base-merged view. `GET` returns **404** ([`AppError`] → `not_found`) when no
//! profile (or scope delta) exists.
//! Every write resolves the workspace identity, applies partial-update
//! semantics (the same as `firetrail profile set`), and leaves the body in
//! `Draft` — confirmation (Draft → Reviewed → Verified) goes through the
//! existing `/api/trust/*` routes, not here.
//!
//! Each handler threads `X-Firetrail-Request-Id` onto the ops input so the
//! matching [`ft_ops::Event::ProfileUpdated`] envelope carries it back to SSE
//! subscribers.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use ft_ops::profile::{self, AddComponentInput, UpdateProfileInput};
use serde::Deserialize;

use crate::error::AppError;
use crate::routes::tickets::resolve_identity;
use crate::server::AppState;

use super::tickets::REQUEST_ID_HEADER;

/// Build the `/api/profile` sub-router. The parent router's session middleware
/// still guards every route.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(get_handler).put(put_handler))
        .route("/resolve", get(resolve_handler))
        .route("/components", post(add_component_handler))
        .route(
            "/components/:name",
            axum::routing::delete(remove_component_handler),
        )
}

/// Query string for `GET`/`PUT /api/profile` — selects a per-scope delta.
///
/// `scope` selects the per-scope record (`owning_scope`); `resolved=1` returns
/// the base-merged view (read-only; ignored on `PUT`). Absent `scope` keeps
/// today's base singleton behaviour byte-for-byte.
#[derive(Debug, Default, Deserialize)]
pub struct ProfileQuery {
    /// Per-scope delta id; `None` selects the base profile.
    #[serde(default)]
    pub scope: Option<String>,
    /// When truthy, return the base-merged (resolved) view on a scoped GET.
    #[serde(default, deserialize_with = "de_truthy")]
    pub resolved: bool,
}

/// Deserialize `resolved=1` / `resolved=true` (and absent) into a bool.
fn de_truthy<'de, D>(de: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(de)?;
    Ok(matches!(raw.as_deref(), Some("1" | "true" | "yes" | "on")))
}

fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}

/// `GET /api/profile[?scope=&resolved=1]` — the current profile, or 404 when
/// none exists.
///
/// With no `scope`, returns the base singleton (404 when absent). With `scope`,
/// returns that scope's stored delta (404 when no delta), or — with
/// `resolved=1` — the base-merged view.
#[tracing::instrument(skip_all)]
pub async fn get_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProfileQuery>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let view = match q.scope.as_deref() {
        Some(scope) => profile::get_for_scope(
            &state.workspace,
            &identity,
            scope,
            q.resolved,
            &state.events,
        )?,
        None => profile::get(&state.workspace, &identity, &state.events)?,
    };
    match view {
        Some(view) => Ok((StatusCode::OK, Json(view))),
        None => Err(AppError::Ops(ft_ops::OpsError::not_found(
            "profile", "<none>",
        ))),
    }
}

/// Query string for `GET /api/profile/resolve` — the changeset to resolve.
///
/// Either an explicit `paths=` list, or `staged=1` to resolve the current
/// staged diff (the "resolve staged diff" button). When `staged` is truthy the
/// `paths` query is ignored; when both are absent the plan is empty.
#[derive(Debug, Default, Deserialize)]
pub struct ResolveQuery {
    /// Comma-separated repo-relative paths.
    #[serde(default)]
    pub paths: Option<String>,
    /// When truthy, resolve the staged diff instead of `paths`.
    #[serde(default, deserialize_with = "de_truthy")]
    pub staged: bool,
}

/// `GET /api/profile/resolve?paths=a,b,c` (or `?staged=1`) — the distinct
/// validate commands the changeset requires, as a
/// [`ft_ops::profile::ValidatePlanView`].
///
/// With `staged=1` the staged diff (`git status` staged entries) is resolved
/// and `paths` is ignored; otherwise the explicit `paths` list is used.
#[tracing::instrument(skip_all)]
pub async fn resolve_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ResolveQuery>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let paths: Vec<PathBuf> = if q.staged {
        ft_git::Repo::open(&state.workspace.root)
            .and_then(|repo| repo.status())
            .map_err(|e| AppError::Internal(anyhow::anyhow!("read staged diff: {e}")))?
            .staged
    } else {
        q.paths
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect()
    };
    let plan = profile::validate_plan(&state.workspace, &identity, &paths, &state.events)?;
    Ok((StatusCode::OK, Json(plan)))
}

/// JSON body for `PUT /api/profile`.
///
/// `Option<Option<T>>` mirrors the ops input: a missing key leaves the field
/// untouched; `null` clears it; a value sets it. A flat `Option` can't express
/// that three-way distinction.
#[allow(clippy::option_option)]
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileBody {
    /// New validate command (`null` clears).
    #[serde(default)]
    pub validate_command: Option<Option<String>>,
    /// New test command.
    #[serde(default)]
    pub test_command: Option<Option<String>>,
    /// New build command.
    #[serde(default)]
    pub build_command: Option<Option<String>>,
    /// New lint command.
    #[serde(default)]
    pub lint_command: Option<Option<String>>,
    /// Replacement language list.
    #[serde(default)]
    pub languages: Option<Vec<String>>,
    /// Replacement package-manager list.
    #[serde(default)]
    pub package_managers: Option<Vec<String>>,
    /// New runtime note.
    #[serde(default)]
    pub runtime: Option<Option<String>>,
    /// New free-form notes.
    #[serde(default)]
    pub notes: Option<Option<String>>,
}

/// `PUT /api/profile[?scope=]` — partial update; creates the record if absent.
///
/// With `scope`, writes the per-scope delta (`owning_scope`); otherwise the base
/// singleton. `resolved` is ignored on a write.
#[tracing::instrument(skip_all)]
pub async fn put_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProfileQuery>,
    headers: HeaderMap,
    body: Option<Json<UpdateProfileBody>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let input = UpdateProfileInput {
        validate_command: body.validate_command,
        test_command: body.test_command,
        build_command: body.build_command,
        lint_command: body.lint_command,
        languages: body.languages,
        package_managers: body.package_managers,
        runtime: body.runtime,
        notes: body.notes,
        request_id: request_id(&headers),
    };
    let view = match q.scope.as_deref() {
        Some(scope) => {
            profile::update_for_scope(&state.workspace, &identity, scope, input, &state.events)?
        }
        None => profile::update(&state.workspace, &identity, input, &state.events)?,
    };
    Ok((StatusCode::OK, Json(view)))
}

/// JSON body for `POST /api/profile/components`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddComponentBody {
    /// Component name (unique key).
    pub name: String,
    /// Repo-relative path.
    pub path: String,
    /// Optional one-line summary.
    #[serde(default)]
    pub summary: Option<String>,
}

/// `POST /api/profile/components` — add (or replace by name) a component.
#[tracing::instrument(skip_all)]
pub async fn add_component_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AddComponentBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let input = AddComponentInput {
        name: body.name,
        path: body.path,
        summary: body.summary,
        request_id: request_id(&headers),
    };
    let view = profile::add_component(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(view)))
}

/// `DELETE /api/profile/components/:name` — remove one component by name.
#[tracing::instrument(skip_all)]
pub async fn remove_component_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let view = profile::remove_component(&state.workspace, &identity, name, &state.events)?;
    Ok((StatusCode::OK, Json(view)))
}
