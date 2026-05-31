//! Integration tests for the ft-ui axum server.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ft_ops::{Event, EventBus};
use ft_ui::server::{AppState, ServerOpts};
use futures_util::StreamExt;
use tempfile::TempDir;
use tokio::net::TcpListener;

/// Bootstrap a workspace marker file inside `dir`.
fn init_workspace(dir: &Path) {
    let ft = dir.join(".firetrail");
    std::fs::create_dir_all(&ft).unwrap();
    std::fs::write(ft.join("config.yml"), b"# firetrail test workspace\n").unwrap();
}

/// Spawn the server on an ephemeral port. Returns the bound addr, state,
/// and a join handle.
async fn spawn_server(workspace_root: &Path, dev: bool) -> (SocketAddr, Arc<AppState>) {
    // Build via the public API path. We replicate the binding part because the
    // public `run` does a graceful-shutdown loop we don't want in tests.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();

    let (state, router) = build_test_state(workspace_root, bound, dev);
    let app = router;

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (bound, state)
}

fn build_test_state(
    workspace_root: &Path,
    bound: SocketAddr,
    dev: bool,
) -> (Arc<AppState>, axum::Router) {
    ft_ui::server::test_app(workspace_root, bound, dev).unwrap()
}

fn client_for(addr: SocketAddr) -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .resolve("localhost", addr)
        .resolve("127.0.0.1", addr)
        .build()
        .unwrap()
}

#[tokio::test]
async fn test_workspace_endpoint_requires_auth() {
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, _state) = spawn_server(tmp.path(), false).await;

    let client = client_for(addr);
    let url = format!("http://{addr}/api/workspace");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_bootstrap_then_workspace() {
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, state) = spawn_server(tmp.path(), false).await;
    let token = state.bootstrap_token.value.clone();

    let client = client_for(addr);

    // 1) Bootstrap with token → 302 + Set-Cookie.
    let url = format!("http://{addr}/?token={token}");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_redirection(),
        "expected redirect, got {}",
        resp.status()
    );
    assert!(resp.headers().get("set-cookie").is_some());

    // 2) Reuse cookie to fetch /api/workspace.
    let url = format!("http://{addr}/api/workspace");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("root").is_some());
}

#[tokio::test]
async fn test_root_no_auth_browser_gets_html() {
    // A human pointing a browser at `/` with no ?token= and no cookie sends
    // `Accept: text/html`. They should get a friendly HTML page telling them
    // to relaunch `firetrail ui`, not raw JSON.
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, _state) = spawn_server(tmp.path(), false).await;

    let client = client_for(addr);
    let url = format!("http://{addr}/");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "browser landing page should be a 200"
    );
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("text/html"),
        "expected text/html content-type for browser navigation, got `{ct}`"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("firetrail ui"),
        "landing page should mention the relaunch command `firetrail ui`; body: {body}"
    );
}

#[tokio::test]
async fn test_root_no_auth_api_client_gets_json_401() {
    // A programmatic client (fetch, curl) sends `Accept: application/json`.
    // The JSON 401 contract must stay intact for them.
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, _state) = spawn_server(tmp.path(), false).await;

    let client = client_for(addr);
    let url = format!("http://{addr}/");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("application/json"),
        "API clients must still get a JSON 401, got content-type `{ct}`"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["kind"], "unauthorized");
}

#[tokio::test]
async fn test_bad_origin_rejected() {
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, state) = spawn_server(tmp.path(), false).await;
    let token = state.bootstrap_token.value.clone();

    let client = client_for(addr);

    // Bootstrap to obtain the session cookie.
    let url = format!("http://{addr}/?token={token}");
    client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();

    // Now request with a hostile Origin.
    let url = format!("http://{addr}/api/workspace");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .header("Origin", "http://evil.com")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_sse_keeps_open() {
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, state) = spawn_server(tmp.path(), false).await;
    let token = state.bootstrap_token.value.clone();

    let client = client_for(addr);
    let url = format!("http://{addr}/?token={token}");
    client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();

    // Open the SSE stream and read for ~2s while we emit an event.
    let url = format!("http://{addr}/api/events");
    let resp = client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let events: EventBus = state.events.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        events.emit(Event::TicketCreated { id: "tk-1".into() });
    });

    let start = Instant::now();
    let mut got_emitted = false;
    let mut stream = resp.bytes_stream();
    while start.elapsed() < Duration::from_secs(2) {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                let s = String::from_utf8_lossy(&chunk);
                if s.contains("event: emitted") && s.contains("tk-1") {
                    got_emitted = true;
                    break;
                }
            }
            Ok(Some(Err(_)) | None) => break,
            Err(_) => {
                // keep-alive tick; loop continues.
            }
        }
    }
    assert!(got_emitted, "expected to see an `emitted` SSE frame");
}

#[tokio::test]
async fn test_heartbeat_keeps_alive() {
    let tmp = TempDir::new().unwrap();
    init_workspace(tmp.path());
    let (addr, state) = spawn_server(tmp.path(), false).await;
    let token = state.bootstrap_token.value.clone();

    let client = client_for(addr);
    let url = format!("http://{addr}/?token={token}");
    client
        .get(&url)
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();

    // Send heartbeats every second for 5s (we don't need to wait 20s — the
    // watchdog is disabled in test mode because we never invoked `run`).
    for _ in 0..5 {
        let url = format!("http://{addr}/api/heartbeat");
        let resp = client
            .post(&url)
            .header("Host", addr.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[test]
fn test_server_opts_parses() {
    // Smoke test that the clap derive accepts the documented flags.
    use clap::Parser;
    let opts = ServerOpts::try_parse_from([
        "ft-ui",
        "--workspace",
        "/tmp/ws",
        "--bind",
        "127.0.0.1:0",
        "--dev",
        "--foreground",
        "--no-open",
    ])
    .unwrap();
    assert!(opts.dev);
    assert!(opts.foreground);
    assert!(opts.no_open);
}
