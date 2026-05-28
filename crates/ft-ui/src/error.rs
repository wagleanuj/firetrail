//! HTTP error type. Wraps [`ft_ops::OpsError`] plus ft-ui-local variants and
//! renders them as `application/json` `{ error: { kind, message, detail } }`.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ft_ops::OpsError;
use serde_json::json;
use thiserror::Error;

/// Top-level error returned from axum handlers.
#[derive(Debug, Error)]
pub enum AppError {
    /// An underlying ops failure.
    #[error(transparent)]
    Ops(#[from] OpsError),

    /// The request did not present a valid session.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// The request's `Host`/`Origin` header was rejected.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Catch-all for internal failures.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    fn parts(&self) -> (StatusCode, &'static str, String, Option<String>) {
        match self {
            Self::Ops(OpsError::NotFound { kind, id }) => (
                StatusCode::NOT_FOUND,
                "not_found",
                format!("{kind} not found: {id}"),
                None,
            ),
            Self::Ops(OpsError::Validation { field, reason }) => (
                StatusCode::BAD_REQUEST,
                "validation",
                format!("validation failed on field `{field}`: {reason}"),
                Some(field.clone()),
            ),
            Self::Ops(OpsError::Conflict { reason }) => (
                StatusCode::CONFLICT,
                "conflict",
                reason.clone(),
                None,
            ),
            Self::Ops(OpsError::PermissionDenied { reason }) => (
                StatusCode::FORBIDDEN,
                "permission_denied",
                reason.clone(),
                None,
            ),
            Self::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                msg.clone(),
                None,
            ),
            Self::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                msg.clone(),
                None,
            ),
            Self::Ops(OpsError::Internal(e)) | Self::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                e.to_string(),
                None,
            ),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, kind, message, detail) = self.parts();
        let body = Json(json!({
            "error": {
                "kind": kind,
                "message": message,
                "detail": detail,
            }
        }));
        (status, body).into_response()
    }
}
