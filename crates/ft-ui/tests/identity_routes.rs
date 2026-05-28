//! Integration tests for the `/api/identity` axum surface (W3-B).

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
    // Bootstrap registry with one identity so `list`/`show` have something
    // to work with even without going through the register endpoint.
    std::fs::write(
        firetrail.join("identities.yaml"),
        "identities:
  - id: alice
    name: Alice Smith
    kind: human
    emails: [alice@firetrail.test]
    status: active
",
    )
    .expect("write identities.yaml");
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

#[tokio::test]
async fn list_returns_seeded_identity() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/identity"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let ids = body["identities"].as_array().expect("identities");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0]["id"], "alice");
}

#[tokio::test]
async fn show_returns_capability_view() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/identity/alice"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["identity"]["id"], "alice");
    assert_eq!(body["identity"]["kind"], "human");
    assert_eq!(body["identity"]["status"], "active");
}

#[tokio::test]
async fn capabilities_returns_effective_matrix() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/identity/alice/capabilities"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["identity"], "alice");
    assert!(body["capabilities"].as_array().is_some());
}

#[tokio::test]
async fn register_then_show_round_trip_threads_request_id() {
    let (addr, _state, client, _tr) = boot().await;

    // Open SSE first.
    let sse_resp = client
        .get(format!("http://{addr}/api/events"))
        .header("Host", addr.to_string())
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(sse_resp.status(), reqwest::StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = client
        .post(format!("http://{addr}/api/identity"))
        .header("Host", addr.to_string())
        .header("X-Firetrail-Request-Id", "idreg-abc")
        .json(&serde_json::json!({
            "id": "bob",
            "name": "Bob",
            "emails": ["bob@firetrail.test"],
            "kind": "human"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    // Show should now find bob.
    let show = client
        .get(format!("http://{addr}/api/identity/bob"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(show.status(), reqwest::StatusCode::OK);

    // SSE should have surfaced an `identity_updated` envelope with the rid.
    let mut stream = sse_resp.bytes_stream();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buf = String::new();
    let mut saw = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if buf.contains("identity_updated") && buf.contains("\"idreg-abc\"") {
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
        "expected identity_updated event tagged with rid=idreg-abc; got:\n{buf}"
    );
}

#[tokio::test]
async fn register_duplicate_is_conflict() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/identity"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "id": "alice",
            "name": "Alice II",
            "emails": ["alice2@firetrail.test"],
            "kind": "human"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CONFLICT);
}

#[tokio::test]
async fn auth_failure_without_cookie_returns_forbidden() {
    let tr = fixture_workspace();
    let (addr, _state) = spawn_server(tr.root()).await;
    let bare = client(addr);
    let resp = bare
        .get(format!("http://{addr}/api/identity"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
