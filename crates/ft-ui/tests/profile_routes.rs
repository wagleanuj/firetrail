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
    std::fs::write(
        firetrail.join("scopes.yaml"),
        "scopes:\n  - id: apps/checkout\n    applies_to: [\"apps/checkout/**\"]\n",
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

// ── Scope-aware routes (Phase 5.2) ───────────────────────────────────────────

#[tokio::test]
async fn get_scope_is_404_when_delta_absent() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/profile?scope=apps/checkout"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn put_scope_partial_updates_delta_then_resolved_merges() {
    let (addr, _state, client, _tr) = boot().await;

    // Seed the base profile (validate only).
    client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "just ci" }))
        .send()
        .await
        .unwrap();

    // PUT ?scope= writes the per-scope delta (test only).
    let resp = client
        .put(format!("http://{addr}/api/profile?scope=apps/checkout"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "testCommand": "pnpm test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["test_command"], "pnpm test");

    // GET ?scope= (raw delta): validate not inherited.
    let resp = client
        .get(format!("http://{addr}/api/profile?scope=apps/checkout"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["test_command"], "pnpm test");
    assert!(body["validate_command"].is_null(), "raw delta: no inherit");

    // GET ?scope=&resolved=1 (merged): validate inherited from base.
    let resp = client
        .get(format!(
            "http://{addr}/api/profile?scope=apps/checkout&resolved=1"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["validate_command"], "just ci");
    assert_eq!(body["test_command"], "pnpm test");
}

#[tokio::test]
async fn resolve_returns_a_validate_plan() {
    let (addr, _state, client, _tr) = boot().await;

    // Base validate + scope override.
    client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "just ci" }))
        .send()
        .await
        .unwrap();
    client
        .put(format!("http://{addr}/api/profile?scope=apps/checkout"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "pnpm --filter checkout test" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .get(format!(
            "http://{addr}/api/profile/resolve?paths=apps/checkout/a.ts,apps/checkout/b.ts,README.md"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let entries = body["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2);
    assert_eq!(body["unresolved"], 0);
    let checkout = entries
        .iter()
        .find(|e| e["command"].as_str().unwrap().contains("checkout"))
        .expect("checkout entry");
    assert_eq!(checkout["fileCount"], 2);
    assert_eq!(checkout["scopes"][0], "apps/checkout");
}

#[tokio::test]
async fn resolve_staged_resolves_the_staged_diff() {
    let (addr, _state, client, tr) = boot().await;

    // Base validate + scope override (same shape as resolve_returns_a_validate_plan).
    client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "just ci" }))
        .send()
        .await
        .unwrap();
    client
        .put(format!("http://{addr}/api/profile?scope=apps/checkout"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "pnpm --filter checkout test" }))
        .send()
        .await
        .unwrap();

    // Stage a file under the apps/checkout scope.
    let root = tr.root();
    std::fs::create_dir_all(root.join("apps/checkout")).expect("mkdir");
    std::fs::write(root.join("apps/checkout/a.ts"), b"export const a = 1;\n").expect("write");
    tr.run("git", &["add", "apps/checkout/a.ts"])
        .expect("git add");

    // ?staged=1 ignores ?paths and resolves the staged diff.
    let resp = client
        .get(format!("http://{addr}/api/profile/resolve?staged=1"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let entries = body["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "only the checkout command applies");
    assert_eq!(entries[0]["command"], "pnpm --filter checkout test");
    assert_eq!(entries[0]["fileCount"], 1);
    assert_eq!(entries[0]["scopes"][0], "apps/checkout");
    assert_eq!(body["unresolved"], 0);
}

#[tokio::test]
async fn resolve_paths_still_works_without_staged() {
    let (addr, _state, client, _tr) = boot().await;
    client
        .put(format!("http://{addr}/api/profile"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "validateCommand": "just ci" }))
        .send()
        .await
        .unwrap();
    let resp = client
        .get(format!("http://{addr}/api/profile/resolve?paths=README.md"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let entries = body["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["command"], "just ci");
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
