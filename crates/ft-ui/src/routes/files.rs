//! `/api/files` HTTP surface — file-path autocomplete for the Profile panel.
//!
//! | Method | Path                              | Op                          | Returns        |
//! |--------|-----------------------------------|-----------------------------|----------------|
//! | GET    | `/api/files[?prefix=&dirs=&limit=]` | [`ft_ops::files::list_files`] | `FileListView` |
//!
//! Read-only. `prefix` filters case-insensitively (empty = all tracked paths);
//! `dirs=1` collapses results to distinct directory prefixes; `limit` caps the
//! count (default 50, clamped by the op to `1..=200`). Ignores the
//! `X-Firetrail-Request-Id` header.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use ft_ops::files::FileListView;
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

/// Default suggestion cap when `limit` is absent.
const DEFAULT_LIMIT: usize = 50;

/// Build the `/api/files` sub-router. The parent router's session middleware
/// still guards every route.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(list_handler))
}

/// Deserialize `dirs=1` / `dirs=true` (and absent) into a bool — mirrors
/// `profile::ProfileQuery::resolved`.
fn de_truthy<'de, D>(de: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(de)?;
    Ok(matches!(raw.as_deref(), Some("1" | "true" | "yes" | "on")))
}

/// Query string for `GET /api/files`.
#[derive(Debug, Default, Deserialize)]
pub struct FilesQuery {
    /// Case-insensitive path prefix; absent/empty matches all tracked paths.
    #[serde(default)]
    pub prefix: Option<String>,
    /// When truthy, collapse results to distinct directory prefixes.
    #[serde(default, deserialize_with = "de_truthy")]
    pub dirs: bool,
    /// Cap on returned suggestions (clamped to `1..=200` by the op).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// `GET /api/files[?prefix=&dirs=1&limit=]` — tracked-path suggestions.
#[tracing::instrument(skip_all)]
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FilesQuery>,
) -> Result<impl IntoResponse, AppError> {
    let prefix = q.prefix.unwrap_or_default();
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    let paths = ft_ops::files::list_files(&state.workspace, &prefix, q.dirs, limit)?;
    Ok((StatusCode::OK, Json(FileListView { paths })))
}
