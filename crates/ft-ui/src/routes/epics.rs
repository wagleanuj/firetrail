//! `/api/epics` HTTP surface — Epic roll-up route for the firetrail GUI.
//!
//! Route table:
//!
//! | Method | Path         | Op                             |
//! |--------|--------------|--------------------------------|
//! | GET    | `/api/epics` | [`ft_ops::tickets::epics`]     |
//!
//! ## Identity
//!
//! Delegates to [`crate::routes::tickets::resolve_identity`] — the same
//! `DefaultResolver` chain used by all other ticket routes.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use ft_ops::tickets::{EpicsInput, epics};

use crate::error::AppError;
use crate::server::AppState;

/// Build the `/api/epics` sub-router.
///
/// Mounted under `/api/epics` by [`crate::routes::build`]. The session
/// middleware applied by the parent router still guards every route.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(epics_handler))
}

/// `GET /api/epics` — epic roll-up snapshot with child counts.
#[tracing::instrument(skip_all)]
pub async fn epics_handler(
    State(state): State<Arc<AppState>>,
    Query(input): Query<EpicsInput>,
) -> Result<impl IntoResponse, AppError> {
    let identity = crate::routes::tickets::resolve_identity(&state.workspace)?;
    let out = epics(&state.workspace, &identity, input, &state.events)?;
    Ok((StatusCode::OK, Json(out)))
}
