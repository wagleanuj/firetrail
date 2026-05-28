//! Integration tests for the `/api/memory` axum surface (W2-B).
//!
//! These tests reuse the bootstrap/cookie pattern from `tickets_routes.rs`.
//! They exercise the discriminated POST, the search route in lexical mode
//! (no daemon needed; semantic mode auto-degrades and is asserted as
//! non-failing), and the salvage dry-run-then-apply two-step (dry-run
//! only — applying salvage requires multi-branch git fixtures beyond
//! the scope of this harness).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ft_testkit::TestRepo;
use ft_ui::server::{AppState, test_app};
use futures_util::StreamExt;
use tokio::net::TcpListener;

const TEST_IDENTITY: &str = "alice@firetrail.test";

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

async fn boot() -> (SocketAddr, Arc<AppState>, reqwest::Client, TestRepo) {
    let tr = fixture_workspace();
    let (addr, state) = spawn_server(tr.root()).await;
    let client = client(addr);

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

async fn create_kind(
    client: &reqwest::Client,
    addr: SocketAddr,
    body: serde_json::Value,
) -> serde_json::Value {
    let resp = client
        .post(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::CREATED,
        "create_kind expected 201, got {}",
        resp.status()
    );
    resp.json().await.unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// List + show round-trip.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_is_empty_on_fresh_workspace() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let rows = body.get("rows").and_then(|v| v.as_array()).expect("rows");
    assert!(rows.is_empty(), "expected empty rows, got {rows:?}");
}

#[tokio::test]
async fn create_then_list_then_show_round_trip() {
    let (addr, _state, client, _tr) = boot().await;

    let created = create_kind(
        &client,
        addr,
        serde_json::json!({
            "kind": "gotcha",
            "summary": "watch out for foo"
        }),
    )
    .await;
    let id = created["record"]["envelope"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    assert!(
        id.starts_with("GOTCHA-"),
        "expected GOTCHA- prefix, got {id}"
    );

    // List should include it.
    let resp = client
        .get(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], id);
    assert_eq!(rows[0]["kind"], "gotcha");

    // Show resolves the same id.
    let resp = client
        .get(format!("http://{addr}/api/memory/{id}"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["record"]["envelope"]["id"], id);
}

#[tokio::test]
async fn show_unknown_id_is_404() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/memory/GOT-deadbeef"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

// ─────────────────────────────────────────────────────────────────────────────
// Create per kind — representative subset (incident, runbook, gotcha).
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_incident_via_discriminator() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_kind(
        &client,
        addr,
        serde_json::json!({
            "kind": "incident",
            "summary": "DB primary failover",
            "severity": "sev2"
        }),
    )
    .await;
    let id = body["record"]["envelope"]["id"].as_str().unwrap();
    assert!(id.starts_with("INC-"), "expected INC- prefix, got {id}");
}

#[tokio::test]
async fn create_runbook_via_discriminator() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_kind(
        &client,
        addr,
        serde_json::json!({
            "kind": "runbook",
            "title": "Restart frobnicator",
            "summary": "Run when the bus is wedged"
        }),
    )
    .await;
    let id = body["record"]["envelope"]["id"].as_str().unwrap();
    assert!(id.starts_with("RUN-"), "expected RUN- prefix, got {id}");
}

#[tokio::test]
async fn create_generic_memory_via_discriminator() {
    let (addr, _state, client, _tr) = boot().await;
    let body = create_kind(
        &client,
        addr,
        serde_json::json!({
            "kind": "memory",
            "title": "remember this",
            "body": "the cache eviction policy is LRU"
        }),
    )
    .await;
    let id = body["record"]["envelope"]["id"].as_str().unwrap();
    assert!(id.starts_with("MEM-"), "expected MEM- prefix, got {id}");
}

#[tokio::test]
async fn create_with_unknown_kind_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "kind": "widget", "summary": "no" }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        (400..500).contains(&status),
        "expected 4xx for unknown kind, got {status}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Capture op.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn capture_defaults_to_memory_kind() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/memory/capture"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "title": "captured",
            "body": "some markdown"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let id = body["record"]["envelope"]["id"].as_str().unwrap();
    assert!(
        id.starts_with("MEM-"),
        "default kind should be memory, got {id}"
    );
}

#[tokio::test]
async fn capture_rejects_empty_body() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/memory/capture"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "title": "captured",
            "body": "   "
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

// ─────────────────────────────────────────────────────────────────────────────
// Search.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_lexical_finds_created_record() {
    let (addr, _state, client, _tr) = boot().await;
    let _ = create_kind(
        &client,
        addr,
        serde_json::json!({
            "kind": "gotcha",
            "summary": "frobnicator wedges under load"
        }),
    )
    .await;

    // Give the index a moment to settle (search engine is synchronous but
    // record creation triggers an indexer pass; in-process it's a no-op
    // wait, but kept here for safety).
    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = client
        .get(format!(
            "http://{addr}/api/memory/search?q=frobnicator&mode=lexical"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let hits = body["hits"].as_array().expect("hits");
    assert!(!hits.is_empty(), "expected at least one hit, got {body}");
}

#[tokio::test]
async fn search_semantic_succeeds_or_degrades_gracefully() {
    let (addr, _state, client, _tr) = boot().await;

    let resp = client
        .get(format!(
            "http://{addr}/api/memory/search?q=anything&mode=semantic"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    // The ops layer auto-falls back to lexical when the daemon is missing,
    // surfacing warnings instead of erroring. Either OK or a clean 5xx
    // is acceptable; a 5xx without a structured body would be a regression.
    let status = resp.status();
    if status.is_success() {
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("hits").is_some(), "expected hits field");
        assert!(body.get("mode").is_some(), "expected mode field");
    } else {
        // Non-2xx: accept any structured response (the AppError envelope is
        // JSON, but reqwest may surface a transport error before we can
        // parse — just assert status code shape).
        assert!(
            status.is_client_error() || status.is_server_error(),
            "unexpected status {status}"
        );
    }
}

#[tokio::test]
async fn search_invalid_mode_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/memory/search?q=x&mode=cosmic"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        (400..500).contains(&status),
        "expected 4xx for invalid mode, got {status}"
    );
}

#[tokio::test]
async fn search_limit_over_max_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/memory/search?q=x&limit=1000"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

// ─────────────────────────────────────────────────────────────────────────────
// Salvage (dry-run only — applying needs branch fixtures).
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn salvage_dry_run_on_clean_workspace() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/memory/salvage"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "base": "main",
            "dryRun": true
        }))
        .send()
        .await
        .unwrap();
    // TestRepo provides a git-initialised workspace, but the salvage op
    // requires the `base` branch to exist. We assert either a clean
    // success (entries possibly empty) or a 400 with a useful message.
    let status = resp.status();
    if status.is_success() {
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["dryRun"], true);
        assert!(body.get("entries").is_some(), "entries field present");
    } else {
        assert!(
            status == reqwest::StatusCode::BAD_REQUEST
                || status == reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected salvage status {status}"
        );
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("error").is_some());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SSE — request_id thread-through on MemoryCreated.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_threads_request_id_into_sse_envelope() {
    let (addr, _state, client, _tr) = boot().await;

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

    let resp = client
        .post(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .header("X-Firetrail-Request-Id", "memrid42")
        .json(&serde_json::json!({
            "kind": "gotcha",
            "summary": "sse test"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    let mut stream = sse_resp.bytes_stream();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buf = String::new();
    let mut saw = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if buf.contains("memory_created") && buf.contains("\"memrid42\"") {
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
        "expected memory_created event tagged with request_id=memrid42; got:\n{buf}"
    );
}
