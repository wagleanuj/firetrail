//! Axum router construction.
//!
//! The router is built in two halves: a public half (`GET /`, `GET /assets/*`)
//! and an authenticated half (`/api/*`) guarded by [`crate::auth::session_middleware`].
//!
//! The `/api/tickets/*` surface lives in [`tickets`].

use std::sync::Arc;

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::auth::{bootstrap_handler, heartbeat_handler, session_middleware, workspace_handler};
use crate::server::AppState;
use crate::sse::events_handler;

pub mod audit;
pub mod identity;
pub mod memory;
pub mod scope;
pub mod tickets;
pub mod trust;

/// Build the top-level axum [`Router`] for ft-ui.
pub fn build(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/workspace", get(workspace_handler))
        .route("/heartbeat", post(heartbeat_handler))
        .route("/events", get(events_handler))
        .nest("/tickets", tickets::router())
        .nest("/memory", memory::router())
        .nest("/scope", scope::router())
        .nest("/identity", identity::router())
        .nest("/trust", trust::router())
        .nest("/audit", audit::router())
        .route_layer(from_fn_with_state(state.clone(), session_middleware))
        .with_state(state.clone());

    Router::new()
        .route("/", get(bootstrap_handler))
        .route("/assets/*path", get(crate::assets::serve))
        .nest("/api", api)
        .with_state(state)
}
