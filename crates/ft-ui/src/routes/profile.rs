//! `/api/profile` HTTP surface — the `RepoProfile` read/edit panel.
//!
//! | Method | Path                              | Op                                  | Returns        |
//! |--------|-----------------------------------|-------------------------------------|----------------|
//! | GET    | `/api/profile`                    | [`ft_ops::profile::get`]            | `ProfileView`  |
//! | PUT    | `/api/profile`                    | [`ft_ops::profile::update`]         | `ProfileView`  |
//! | POST   | `/api/profile/components`         | [`ft_ops::profile::add_component`]  | `ProfileView`  |
//! | DELETE | `/api/profile/components/:name`   | [`ft_ops::profile::remove_component`] | `ProfileView`|
//!
//! `GET` returns **404** ([`AppError`] → `not_found`) when no profile exists.
//! Every write resolves the workspace identity, applies partial-update
//! semantics (the same as `firetrail profile set`), and leaves the body in
//! `Draft` — confirmation (Draft → Reviewed → Verified) goes through the
//! existing `/api/trust/*` routes, not here.
//!
//! Each handler threads `X-Firetrail-Request-Id` onto the ops input so the
//! matching [`ft_ops::Event::ProfileUpdated`] envelope carries it back to SSE
//! subscribers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
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
        .route("/components", post(add_component_handler))
        .route(
            "/components/:name",
            axum::routing::delete(remove_component_handler),
        )
}

fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}

/// `GET /api/profile` — the current profile, or 404 when none exists.
#[tracing::instrument(skip_all)]
pub async fn get_handler(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    match profile::get(&state.workspace, &identity, &state.events)? {
        Some(view) => Ok((StatusCode::OK, Json(view))),
        None => Err(AppError::Ops(ft_ops::OpsError::not_found(
            "profile", "<none>",
        ))),
    }
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

/// `PUT /api/profile` — partial update; creates the profile if absent.
#[tracing::instrument(skip_all)]
pub async fn put_handler(
    State(state): State<Arc<AppState>>,
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
    let view = profile::update(&state.workspace, &identity, input, &state.events)?;
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
