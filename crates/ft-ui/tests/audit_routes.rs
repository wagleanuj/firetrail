//! Integration tests for the `/api/audit` axum surface (W3-B).

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

async fn create_task(client: &reqwest::Client, addr: SocketAddr, title: &str) -> String {
    let resp = client
        .post(format!("http://{addr}/api/tickets"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "kind": "task", "title": title }))
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
async fn lint_on_empty_workspace_returns_ok() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/audit/lint"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("scanned").is_some());
    assert!(body.get("findings").is_some());
}

#[tokio::test]
async fn verify_on_empty_workspace_reports_zero() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/audit/verify"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["failures"], 0);
}

#[tokio::test]
async fn criteria_add_then_list_then_toggle_round_trip_with_request_id() {
    let (addr, _state, client, _tr) = boot().await;

    // SSE first.
    let sse_resp = client
        .get(format!("http://{addr}/api/events"))
        .header("Host", addr.to_string())
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(sse_resp.status(), reqwest::StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let id = create_task(&client, addr, "do the thing").await;

    let resp = client
        .post(format!("http://{addr}/api/audit/criteria/{id}"))
        .header("Host", addr.to_string())
        .header("X-Firetrail-Request-Id", "ac-rid-1")
        .json(&serde_json::json!({ "text": "first criterion" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    // List should show one row, unchecked.
    let list = client
        .get(format!("http://{addr}/api/audit/criteria/{id}"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = list.json().await.unwrap();
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["checked"], false);
    let which = items[0]["id"].as_str().unwrap().to_string();

    // Toggle to checked.
    let resp = client
        .patch(format!("http://{addr}/api/audit/criteria/{id}/{which}"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "checked": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // Re-list confirms checked.
    let list = client
        .get(format!("http://{addr}/api/audit/criteria/{id}"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = list.json().await.unwrap();
    assert_eq!(body["items"][0]["checked"], true);

    // SSE should have surfaced a ticket_updated tagged with the rid.
    let mut stream = sse_resp.bytes_stream();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buf = String::new();
    let mut saw = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if buf.contains("ticket_updated") && buf.contains("\"ac-rid-1\"") {
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
        "expected ticket_updated event tagged with rid=ac-rid-1; got:\n{buf}"
    );
}

#[tokio::test]
async fn criteria_evidence_attaches_url() {
    let (addr, _state, client, _tr) = boot().await;
    let id = create_task(&client, addr, "evidence task").await;
    let _ = client
        .post(format!("http://{addr}/api/audit/criteria/{id}"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "text": "needs proof" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("http://{addr}/api/audit/criteria/{id}/1/evidence"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "url": "https://example.com/pr/1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn graph_depth_over_max_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!(
            "http://{addr}/api/audit/graph?id=GOTCHA-deadbeef&depth=99"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn review_unknown_record_is_404_or_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/audit/review/MEM-deadbeef"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    let status = resp.status();
    assert!(
        status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::BAD_REQUEST,
        "expected 404/400 for unknown record, got {status}"
    );
}

#[tokio::test]
async fn auth_failure_without_cookie_returns_forbidden() {
    let tr = fixture_workspace();
    let (addr, _state) = spawn_server(tr.root()).await;
    let bare = client(addr);
    let resp = bare
        .post(format!("http://{addr}/api/audit/lint"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
