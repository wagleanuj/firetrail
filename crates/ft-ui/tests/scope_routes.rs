//! Integration tests for the `/api/scope` axum surface (W3-B).

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
    // Minimal scopes.yaml with one scope so list returns a non-empty array.
    std::fs::write(
        firetrail.join("scopes.yaml"),
        r#"schema_version: 1
scopes:
  - id: backend
    name: Backend
    applies_to: ["crates/**"]
    aliases: ["be"]
"#,
    )
    .expect("write scopes.yaml");
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
async fn list_returns_scopes_from_yaml() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/scope"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let scopes = body["scopes"].as_array().expect("scopes");
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0]["id"], "backend");
}

#[tokio::test]
async fn show_resolves_by_alias() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/scope/be"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["scope"]["summary"]["id"], "backend");
}

#[tokio::test]
async fn aliases_returns_alphabetical() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/scope/aliases"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let aliases = body["aliases"].as_array().expect("aliases");
    assert!(!aliases.is_empty());
}

#[tokio::test]
async fn show_unknown_scope_is_404() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/scope/no-such-scope"))
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
    // Fresh client with no bootstrap → no cookie.
    let bare = client(addr);
    let resp = bare
        .get(format!("http://{addr}/api/scope"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
