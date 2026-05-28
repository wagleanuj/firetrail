//! Authentication: bootstrap token + signed session cookie + Origin/Host checks.
//!
//! Flow:
//! 1. The `ft ui` subcommand prints `http://127.0.0.1:PORT/?token=...` and
//!    opens the browser at that URL.
//! 2. [`bootstrap_handler`] validates the token against [`SingleUseToken`],
//!    sets a signed `firetrail_session` cookie, redirects to `/` (stripping
//!    the token from the URL), and serves `index.html` thereafter.
//! 3. [`session_middleware`] guards `/api/*`: it verifies the cookie
//!    signature, the `Host` header (must equal the bound `host:port`), and
//!    the `Origin` header (absent, or matching the bound origin). In `--dev`
//!    mode the Vite dev origin is additionally allowed.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use cookie::{Cookie, CookieJar, Key, SameSite};
use serde::Deserialize;

use crate::error::AppError;
use crate::server::AppState;

/// Name of the signed session cookie.
pub const SESSION_COOKIE: &str = "firetrail_session";
/// Lifetime of the bootstrap token after server start.
pub const BOOTSTRAP_TTL: Duration = Duration::from_secs(60);
/// Lifetime of the session cookie.
pub const SESSION_TTL_SECONDS: i64 = 86_400;

/// A bootstrap token that can be redeemed at most once and only within
/// [`BOOTSTRAP_TTL`] of `created_at`.
#[derive(Debug)]
pub struct SingleUseToken {
    /// Base64-url encoded random token.
    pub value: String,
    created_at: Instant,
    redeemed: Mutex<bool>,
}

impl SingleUseToken {
    /// Construct a fresh token.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self {
            value,
            created_at: Instant::now(),
            redeemed: Mutex::new(false),
        }
    }

    /// Attempt to redeem the token. Returns `Ok(())` on success.
    pub fn redeem(&self, candidate: &str) -> Result<(), &'static str> {
        if self.created_at.elapsed() > BOOTSTRAP_TTL {
            return Err("bootstrap token expired");
        }
        if !constant_time_eq(candidate.as_bytes(), self.value.as_bytes()) {
            return Err("bootstrap token mismatch");
        }
        let mut redeemed = self.redeemed.lock().expect("token mutex poisoned");
        if *redeemed {
            return Err("bootstrap token already redeemed");
        }
        *redeemed = true;
        Ok(())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

/// Query string accepted by [`bootstrap_handler`].
#[derive(Debug, Deserialize)]
pub struct BootstrapQuery {
    /// The single-use bootstrap token.
    pub token: Option<String>,
}

/// GET `/` — bootstrap entrypoint.
///
/// Behaviour:
/// - If `?token=` is present and valid: set the session cookie, 302 → `/`.
/// - If `?token=` is missing and the session cookie is valid: serve the SPA.
/// - Otherwise: 401.
#[tracing::instrument(skip_all)]
pub async fn bootstrap_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<BootstrapQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(token) = q.token.as_deref() {
        state
            .bootstrap_token
            .redeem(token)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;
        let cookie = build_session_cookie(&state.session_key);
        let mut response = Redirect::to("/").into_response();
        response
            .headers_mut()
            .insert(header::SET_COOKIE, cookie.parse().expect("cookie header"));
        return Ok(response);
    }

    if has_valid_session(&headers, &state.session_key) {
        return Ok(crate::assets::serve_index());
    }

    Err(AppError::Unauthorized(
        "missing or invalid session".into(),
    ))
}

/// Build the signed `Set-Cookie` value for the session.
fn build_session_cookie(key: &Key) -> String {
    let mut cookie = Cookie::new(SESSION_COOKIE, "ok");
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Strict);
    cookie.set_path("/");
    cookie.set_max_age(cookie::time::Duration::seconds(SESSION_TTL_SECONDS));

    let mut jar = CookieJar::new();
    jar.signed_mut(key).add(cookie);
    jar.get(SESSION_COOKIE)
        .expect("just inserted")
        .to_string()
}

/// Verify a signed session cookie on the request.
pub fn has_valid_session(headers: &HeaderMap, key: &Key) -> bool {
    let Some(raw) = headers.get(header::COOKIE) else {
        return false;
    };
    let Ok(raw) = raw.to_str() else {
        return false;
    };
    let mut jar = CookieJar::new();
    for part in raw.split(';') {
        if let Ok(c) = Cookie::parse(part.trim().to_string()) {
            jar.add_original(c);
        }
    }
    jar.signed(key).get(SESSION_COOKIE).is_some()
}

/// Middleware guarding the `/api/*` routes.
#[tracing::instrument(skip_all)]
pub async fn session_middleware(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let headers = req.headers().clone();

    if !has_valid_session(&headers, &state.session_key) {
        return Err(AppError::Forbidden("invalid session cookie".into()));
    }

    let expected_origin = format!("http://{}", state.bound_addr);
    let expected_host = state.bound_addr.to_string();

    let host_ok = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| h == expected_host);
    if !host_ok {
        return Err(AppError::Forbidden(format!(
            "Host header must equal {expected_host}"
        )));
    }

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let allowed = origin == expected_origin
            || (state.dev_mode
                && (origin == "http://127.0.0.1:5173" || origin == "http://localhost:5173"));
        if !allowed {
            return Err(AppError::Forbidden(format!(
                "Origin `{origin}` not permitted"
            )));
        }
    }

    Ok(next.run(req).await)
}

/// Stub `/api/heartbeat` handler. Returns 204.
#[tracing::instrument(skip_all)]
pub async fn heartbeat_handler(State(state): State<Arc<AppState>>) -> StatusCode {
    *state.last_heartbeat.lock().expect("heartbeat mutex") = Instant::now();
    state
        .heartbeat_seen
        .store(true, std::sync::atomic::Ordering::SeqCst);
    StatusCode::NO_CONTENT
}

/// Stub `/api/workspace` handler. Returns minimal workspace info for Wave 0.
///
/// `identity` is resolved via [`ft_identity::DefaultResolver`] — the same
/// chain ft-cli uses (`FIRETRAIL_AUTHOR` env → workspace identity config →
/// git config `user.email`). Returning `None` here implies *no* identity is
/// resolvable, not "we did not look".
#[tracing::instrument(skip_all)]
pub async fn workspace_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    use ft_identity::{DefaultResolver, IdentityResolver};
    let identity = DefaultResolver::new(&state.workspace.root, false)
        .resolve()
        .ok()
        .map(|core| core.as_str().to_string());
    let info = serde_json::json!({
        "name": state.workspace.root.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(""),
        "root": state.workspace.root,
        "identity": identity,
    });
    Json(info)
}
