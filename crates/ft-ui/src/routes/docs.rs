//! `/api/docs` HTTP surface — doc edit-through for the ticket-drawer Docs panel
//! (firetrail-2mwp.8).
//!
//! | Method | Path                     | Op                       |
//! |--------|--------------------------|--------------------------|
//! | PUT    | `/api/docs/:id/content`  | [`ft_ops::docs::edit`]   |
//!
//! Editing a doc here writes the new markdown through to the backing `.md`
//! file, re-derives `content_hash` + summary, persists the record, and
//! re-indexes synchronously — so a save flips a stale badge back to fresh
//! without any out-of-band `firetrail doc index`. The ticket-scoped *read*
//! (`GET /api/tickets/:id/docs`) lives in [`crate::routes::tickets`].

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::put,
};
use ft_ops::docs::EditDocInput;
use serde::Deserialize;

use crate::error::AppError;
use crate::routes::tickets::resolve_identity;
use crate::server::AppState;

/// Build the `/api/docs` sub-router. The session middleware applied by the
/// parent router still guards every route.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/:id/content", put(edit_handler))
}

/// JSON body for `PUT /api/docs/:id/content`.
#[derive(Debug, Deserialize)]
pub struct EditDocBody {
    /// Full new markdown content to write through to the file.
    pub content: String,
}

/// `PUT /api/docs/:id/content` — write new content through to the file and
/// re-index. Returns the refreshed [`ft_ops::docs::DocView`].
#[tracing::instrument(skip_all)]
pub async fn edit_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<EditDocBody>,
) -> Result<impl IntoResponse, AppError> {
    let identity = resolve_identity(&state.workspace)?;
    let out = ft_ops::docs::edit(
        &state.workspace,
        &identity,
        EditDocInput {
            id,
            content: body.content,
        },
        &state.events,
    )?;
    Ok((StatusCode::OK, Json(out)))
}
