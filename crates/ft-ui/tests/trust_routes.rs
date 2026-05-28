//! Integration tests for the `/api/trust` axum surface (W3-B).

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

async fn create_gotcha(client: &reqwest::Client, addr: SocketAddr, summary: &str) -> String {
    let resp = client
        .post(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "kind": "gotcha",
            "summary": summary
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn review_transitions_draft_to_reviewed_and_threads_request_id() {
    let (addr, _state, client, tr) = boot().await;

    let sse_resp = client
        .get(format!("http://{addr}/api/events"))
        .header("Host", addr.to_string())
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(sse_resp.status(), reqwest::StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let id = create_gotcha(&client, addr, "trust me").await;

    // Swap workspace identity so the review is performed by a *different*
    // actor than the one who created the gotcha — ft_trust forbids
    // self-review.
    std::fs::write(
        tr.firetrail_dir().join("identity.yml"),
        "email: reviewer@firetrail.test\n",
    )
    .expect("rewrite identity.yml");

    let resp = client
        .post(format!("http://{addr}/api/trust/{id}/review"))
        .header("Host", addr.to_string())
        .header("X-Firetrail-Request-Id", "trust-rid-1")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::OK, "review failed: {body}");
    // TrustOutput.record body carries the new trust state.
    assert!(body["record"].is_object());

    // SSE should have surfaced trust_transitioned with the rid.
    let mut stream = sse_resp.bytes_stream();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buf = String::new();
    let mut saw = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if buf.contains("trust_transitioned") && buf.contains("\"trust-rid-1\"") {
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
        "expected trust_transitioned event tagged with rid=trust-rid-1; got:\n{buf}"
    );
}

#[tokio::test]
async fn promote_on_draft_is_conflict() {
    // Draft → Verified is not allowed; ops returns Conflict.
    let (addr, _state, client, _tr) = boot().await;
    let id = create_gotcha(&client, addr, "needs review first").await;
    let resp = client
        .post(format!("http://{addr}/api/trust/{id}/promote"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    // ft_trust validates and returns either Conflict (record is in
    // unexpected state) or Validation (transition rejected).
    let status = resp.status();
    assert!(
        status == reqwest::StatusCode::CONFLICT || status == reqwest::StatusCode::BAD_REQUEST,
        "expected 409 or 400 on illegal promote, got {status}"
    );
}

#[tokio::test]
async fn archive_on_existing_record_succeeds() {
    let (addr, _state, client, _tr) = boot().await;
    let id = create_gotcha(&client, addr, "archive me").await;
    let resp = client
        .post(format!("http://{addr}/api/trust/{id}/archive"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn deprecate_requires_reason() {
    let (addr, _state, client, _tr) = boot().await;
    let id = create_gotcha(&client, addr, "no-reason").await;
    let resp = client
        .post(format!("http://{addr}/api/trust/{id}/deprecate"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    // Body without `reason` should be a 400/422 from axum's Json extractor.
    let status = resp.status().as_u16();
    assert!(
        (400..500).contains(&status),
        "expected 4xx when reason missing, got {status}"
    );
}

#[tokio::test]
async fn auth_failure_without_cookie_returns_forbidden() {
    let tr = fixture_workspace();
    let (addr, _state) = spawn_server(tr.root()).await;
    let bare = client(addr);
    let resp = bare
        .post(format!("http://{addr}/api/trust/GOTCHA-deadbeef/review"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
