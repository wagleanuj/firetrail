//! Integration tests for the `/api/tickets` axum surface (W1-B).
//!
//! Each test spins up an in-process server bound to an ephemeral port,
//! bootstraps the session cookie, and then exercises one of the ticket
//! routes. We avoid the binary boundary deliberately: cross-process
//! event delivery is already covered by `tests/e2e_smoke.rs`; here we
//! care about request → ops → response correctness, including the
//! `X-Firetrail-Request-Id` round-trip on SSE.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ft_testkit::TestRepo;
use ft_ui::server::{AppState, test_app};
use futures_util::StreamExt;
use tokio::net::TcpListener;

const TEST_IDENTITY: &str = "alice@firetrail.test";

/// Build a workspace fixture with `.firetrail/config.yml` (strict=false)
/// and a static `.firetrail/identity.yml` so [`DefaultResolver`] doesn't
/// need to consult environment variables (which would force us into an
/// `unsafe` block under edition 2024).
fn fixture_workspace() -> TestRepo {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).expect("mkdir .firetrail");
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .expect("write config.yml");
    std::fs::write(
        firetrail.join("identity.yml"),
        format!("email: {TEST_IDENTITY}\n"),
    )
    .expect("write identity.yml");
    tr
}

async fn spawn_server(workspace_root: &Path) -> (SocketAddr, Arc<AppState>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let (state, router) = test_app(workspace_root, bound, false).expect("test_app");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (bound, state)
}

fn client(addr: SocketAddr) -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .resolve("localhost", addr)
        .resolve("127.0.0.1", addr)
        .build()
        .unwrap()
}

/// Spin up a server, bootstrap the session, and return `(addr, state, client)`.
async fn boot() -> (SocketAddr, Arc<AppState>, reqwest::Client, TestRepo) {
    let tr = fixture_workspace();
    let (addr, state) = spawn_server(tr.root()).await;
    let client = client(addr);

    // Redeem bootstrap → session cookie in the jar.
    let token = state.bootstrap_token.value.clone();
    let resp = client
        .get(format!("http://{addr}/?token={token}"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "bootstrap status");

    (addr, state, client, tr)
}

async fn create_task(client: &reqwest::Client, addr: SocketAddr, title: &str) -> serde_json::Value {
    let resp = client
        .post(format!("http://{addr}/api/tickets"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "kind": "task",
            "title": title,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "create_task");
    resp.json().await.unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// Read paths.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn board_returns_empty_columns_on_fresh_workspace() {
    let (addr, _state, client, _tr) = boot().await;

    let resp = client
        .get(format!("http://{addr}/api/tickets/board"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    for col in ["todo", "in_progress", "review", "done"] {
        assert!(
            body.get(col).and_then(|v| v.as_array()).is_some(),
            "col {col}"
        );
    }
}

#[tokio::test]
async fn list_filters_via_query_params() {
    let (addr, _state, client, _tr) = boot().await;
    let _ = create_task(&client, addr, "first").await;

    let resp = client
        .get(format!("http://{addr}/api/tickets?status=open"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let rows = body.get("rows").and_then(|v| v.as_array()).expect("rows");
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn show_unknown_ticket_is_404() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/tickets/TASK-deadbeef"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

// ─────────────────────────────────────────────────────────────────────────────
// Create / update / close.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_task_via_discriminator_returns_201() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_task(&client, addr, "hello").await;
    let id = body
        .get("record")
        .and_then(|r| r.get("envelope"))
        .and_then(|e| e.get("id"))
        .and_then(|v| v.as_str())
        .expect("id");
    assert!(id.starts_with("TASK-"), "expected TASK-prefix id, got {id}");
}

#[tokio::test]
async fn create_with_unknown_kind_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/tickets"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "kind": "widget", "title": "no" }))
        .send()
        .await
        .unwrap();
    // axum's Json extractor returns 422/400 for malformed body; both are valid.
    let status = resp.status().as_u16();
    assert!(
        (400..500).contains(&status),
        "expected 4xx for unknown kind, got {status}"
    );
}

#[tokio::test]
async fn patch_updates_title() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_task(&client, addr, "old").await;
    let id = body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = client
        .patch(format!("http://{addr}/api/tickets/{id}"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "title": "new" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["record"]["envelope"]["title"], "new");
}

#[tokio::test]
async fn patch_with_empty_body_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_task(&client, addr, "x").await;
    let id = body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = client
        .patch(format!("http://{addr}/api/tickets/{id}"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn close_twice_yields_conflict() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_task(&client, addr, "to-close").await;
    let id = body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/close"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "first close");

    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/close"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CONFLICT, "second close");
}

#[tokio::test]
async fn reopen_after_close_round_trip() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_task(&client, addr, "to-reopen").await;
    let id = body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Close first.
    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/close"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "close");

    // Reopen.
    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/reopen"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "reopen");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["record"]["envelope"]["status"], "open");

    // Reopening an already-open ticket is a conflict.
    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/reopen"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::CONFLICT,
        "reopen non-closed"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Links.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn link_round_trip_via_show() {
    let (addr, _state, client, _tr) = boot().await;
    let a = create_task(&client, addr, "a").await;
    let b = create_task(&client, addr, "b").await;
    let a_id = a["record"]["envelope"]["id"].as_str().unwrap().to_string();
    let b_id = b["record"]["envelope"]["id"].as_str().unwrap().to_string();

    let resp = client
        .post(format!("http://{addr}/api/tickets/{a_id}/links"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "to": b_id, "kind": "blocks" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .get(format!("http://{addr}/api/tickets/{a_id}"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let rels = body["relations"].as_array().expect("relations array");
    assert_eq!(rels.len(), 1, "show should surface 1 link, body = {body}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Claim / unclaim with SSE request-id correlation.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn claim_threads_request_id_into_sse_envelope() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_task(&client, addr, "claimable").await;
    let id = body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Open SSE first so we don't miss the emission.
    let sse_resp = client
        .get(format!("http://{addr}/api/events"))
        .header("Host", addr.to_string())
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(sse_resp.status(), reqwest::StatusCode::OK);

    // Give the broadcast subscription a moment to attach.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // POST claim with the correlation header.
    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/claim"))
        .header("Host", addr.to_string())
        .header("X-Firetrail-Request-Id", "deadbeef")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // Read the SSE stream until we see the claim event tagged with our id.
    let mut stream = sse_resp.bytes_stream();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buf = String::new();
    let mut saw = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if buf.contains("ticket_claimed") && buf.contains("\"deadbeef\"") {
                    saw = true;
                    break;
                }
            }
            Ok(Some(Err(_)) | None) => break,
            Err(_) => {}
        }
    }
    assert!(
        saw,
        "expected ticket_claimed event tagged with request_id=deadbeef; got:\n{buf}"
    );
}
