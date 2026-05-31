//! Integration tests for the `/api/profile` axum surface (`RepoProfile` epic).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use ft_testkit::TestRepo;
use ft_ui::server::{AppState, test_app};
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

#[tokio::test]
async fn get_profile_is_404_when_absent() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn put_creates_then_get_round_trips() {
    let (addr, _state, client, _tr) = boot().await;

    let resp = client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "validateCommand": "cargo test && cargo clippy",
            "languages": ["rust", "typescript"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["validate_command"], "cargo test && cargo clippy");
    assert_eq!(body["languages"][0], "rust");
    assert_eq!(body["trust"], "draft");

    // GET now returns the profile.
    let resp = client
        .get(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["validate_command"], "cargo test && cargo clippy");
}

#[tokio::test]
async fn put_partial_update_preserves_untouched() {
    let (addr, _state, client, _tr) = boot().await;
    // Seed.
    client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "just ci", "testCommand": "cargo test" }))
        .send()
        .await
        .unwrap();
    // Update only the build command.
    let resp = client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "buildCommand": "cargo build" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["build_command"], "cargo build");
    assert_eq!(body["validate_command"], "just ci");
    assert_eq!(body["test_command"], "cargo test");
}

#[tokio::test]
async fn component_add_and_delete() {
    let (addr, _state, client, _tr) = boot().await;

    let resp = client
        .post(format!("http://{addr}/api/profile/components"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "name": "ft-ui",
            "path": "crates/ft-ui",
            "summary": "web UI"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["components"].as_array().unwrap().len(), 1);
    assert_eq!(body["components"][0]["name"], "ft-ui");

    // Delete it.
    let resp = client
        .delete(format!("http://{addr}/api/profile/components/ft-ui"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["components"].as_array().unwrap().len(), 0);

    // Deleting again is a 404 (component gone).
    let resp = client
        .delete(format!("http://{addr}/api/profile/components/ft-ui"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_failure_without_cookie_returns_forbidden() {
    let tr = fixture_workspace();
    let (addr, _state) = spawn_server(tr.root()).await;
    let bare = client(addr);
    let resp = bare
        .get(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
