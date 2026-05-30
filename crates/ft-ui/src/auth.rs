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

    // No token, no valid cookie. If this looks like a browser navigation
    // (`Accept: text/html`), serve a friendly landing page instead of raw
    // JSON — a human who bookmarked the URL or whose 24h cookie expired
    // should be told how to re-bootstrap, not handed an API error blob.
    // Programmatic clients (fetch/curl with `Accept: application/json` or
    // a bare `*/*`) keep getting the JSON 401 so nothing automated breaks.
    if accepts_html(&headers) {
        return Ok(rebootstrap_landing_page());
    }

    Err(AppError::Unauthorized("missing or invalid session".into()))
}

/// True if the request's `Accept` header explicitly lists `text/html`,
/// i.e. it looks like a top-level browser navigation rather than a
/// programmatic fetch. A bare `*/*` (curl's default) does NOT match.
fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| {
            accept
                .split(',')
                .any(|part| part.trim().split(';').next().unwrap_or("").trim() == "text/html")
        })
}

/// A minimal, self-contained HTML page shown to browsers that reach `/`
/// without a valid bootstrap token or session. Served as `200 text/html`:
/// it is a navigable landing page, not an error condition for the user, so
/// a 200 matches the "friendly re-bootstrap path" intent of the issue.
fn rebootstrap_landing_page() -> Response {
    const PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>firetrail — session expired</title>
<style>
  :root { color-scheme: light dark; }
  body {
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    font: 16px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    background: #f6f7f9;
    color: #1b1f24;
  }
  @media (prefers-color-scheme: dark) {
    body { background: #0d1117; color: #e6edf3; }
    .card { background: #161b22; border-color: #30363d; }
    code { background: #21262d; }
  }
  .card {
    max-width: 28rem;
    margin: 1.5rem;
    padding: 2rem;
    background: #fff;
    border: 1px solid #d0d7de;
    border-radius: 12px;
    box-shadow: 0 1px 3px rgba(0,0,0,0.08);
  }
  h1 { font-size: 1.25rem; margin: 0 0 0.75rem; }
  p { margin: 0 0 1rem; }
  code {
    background: #f0f1f3;
    padding: 0.15rem 0.4rem;
    border-radius: 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.95em;
  }
  .muted { color: #57606a; font-size: 0.9rem; margin-bottom: 0; }
  @media (prefers-color-scheme: dark) { .muted { color: #8b949e; } }
</style>
</head>
<body>
  <main class="card">
    <h1>Your firetrail session has expired</h1>
    <p>This page needs a fresh bootstrap link. Your one-time token was already used, or your session cookie expired.</p>
    <p>To get a new link, relaunch the UI from your terminal:</p>
    <p><code>firetrail ui</code></p>
    <p class="muted">It will open a fresh tab automatically. The port changes on each launch, so this bookmarked URL won't work on its own.</p>
  </main>
</body>
</html>
"#;
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        PAGE,
    )
        .into_response()
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
    jar.get(SESSION_COOKIE).expect("just inserted").to_string()
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
