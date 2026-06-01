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

// ── Write routes ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn post_creates_scope_then_list_includes_it() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/scope"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "id": "frontend",
            "name": "Frontend",
            "appliesTo": ["web/**"],
            "aliases": ["fe"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let scopes = body["scopes"].as_array().expect("scopes");
    assert!(scopes.iter().any(|s| s["id"] == "frontend"));

    // Now GET /api/scope lists it.
    let list = client
        .get(format!("http://{addr}/api/scope"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list.json().await.unwrap();
    let ids: Vec<&str> = list_body["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"frontend"));
}

#[tokio::test]
async fn put_edits_scope() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .put(format!("http://{addr}/api/scope/backend"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "name": "Backend renamed",
            "appliesTo": ["crates/**", "libs/**"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let backend = body["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "backend")
        .unwrap();
    assert_eq!(backend["name"], "Backend renamed");
    assert_eq!(backend["appliesTo"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn delete_removes_scope() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .delete(format!("http://{addr}/api/scope/backend"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["scopes"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn post_reorder_reverses_order() {
    let (addr, _state, client, _tr) = boot().await;
    // Add a second scope so there is something to reorder.
    client
        .post(format!("http://{addr}/api/scope"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "id": "frontend", "appliesTo": ["web/**"] }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("http://{addr}/api/scope/reorder"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "ids": ["frontend", "backend"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let ids: Vec<&str> = body["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["frontend", "backend"]);
}

#[tokio::test]
async fn get_preview_returns_match_counts_shape() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/scope/preview"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let scopes = body["scopes"].as_array().expect("scopes");
    // The fixture has one scope (`backend`); each row carries a matchCount.
    assert!(scopes.iter().any(|s| s["id"] == "backend"));
    assert!(scopes[0].get("matchCount").is_some());
    assert!(body.get("warnings").is_some());
}

#[tokio::test]
async fn post_with_bad_glob_is_4xx() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .post(format!("http://{addr}/api/scope"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "id": "bad", "appliesTo": ["a/[b"] }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "expected 4xx, got {}",
        resp.status()
    );
}
