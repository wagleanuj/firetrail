//! Integration tests for the unified `/api/search` axum surface
//! (firetrail-8z0m.4).
//!
//! These tests reuse the bootstrap/cookie pattern from `memory_routes.rs` /
//! `tickets_routes.rs`. They seed a task (work-tracking domain) and a memory
//! (gotcha) and assert that a single cross-domain query returns hits from
//! *both* kinds, each carrying a populated `kind` + `trust` field — the core
//! acceptance bullet for the global search route.

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

async fn create_task(client: &reqwest::Client, addr: SocketAddr, title: &str) -> String {
    let resp = client
        .post(format!("http://{addr}/api/tickets"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "kind": "task", "title": title }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "create_task");
    let body: serde_json::Value = resp.json().await.unwrap();
    body["record"]["envelope"]["id"].as_str().unwrap().to_string()
}

async fn create_memory(client: &reqwest::Client, addr: SocketAddr, summary: &str) -> String {
    let resp = client
        .post(format!("http://{addr}/api/memory"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({ "kind": "gotcha", "summary": summary }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "create_memory");
    let body: serde_json::Value = resp.json().await.unwrap();
    body["record"]["envelope"]["id"].as_str().unwrap().to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Cross-domain search.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_returns_hits_across_task_and_memory_kinds() {
    let (addr, _state, client, _tr) = boot().await;

    // Seed one record in each of two distinct domains, both matching the same
    // distinctive token so a single query surfaces both.
    let task_id = create_task(&client, addr, "frobnicator throughput regression").await;
    let mem_id = create_memory(&client, addr, "frobnicator wedges under load").await;

    let resp = client
        .get(format!(
            "http://{addr}/api/search?q=frobnicator&mode=lexical"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "search status");
    let body: serde_json::Value = resp.json().await.unwrap();
    let hits = body["hits"].as_array().expect("hits array");
    assert!(!hits.is_empty(), "expected hits, got {body}");

    // Every hit must carry a non-empty kind + trust (acceptance bullet 3).
    for hit in hits {
        assert!(
            hit["kind"].as_str().is_some_and(|k| !k.is_empty()),
            "hit missing kind: {hit}"
        );
        assert!(
            hit["trust"].as_str().is_some_and(|t| !t.is_empty()),
            "hit missing trust: {hit}"
        );
        // `scope` and `mode` keys are always present (scope may be null).
        assert!(hit.get("scope").is_some(), "hit missing scope key: {hit}");
        assert!(hit.get("mode").is_some(), "hit missing mode key: {hit}");
    }

    let kinds: Vec<&str> = hits.iter().filter_map(|h| h["kind"].as_str()).collect();
    assert!(kinds.contains(&"task"), "expected a task hit, kinds={kinds:?}");
    assert!(
        kinds.contains(&"gotcha"),
        "expected a gotcha (memory) hit, kinds={kinds:?}"
    );

    let ids: Vec<&str> = hits.iter().filter_map(|h| h["id"].as_str()).collect();
    assert!(ids.contains(&task_id.as_str()), "task id not in {ids:?}");
    assert!(ids.contains(&mem_id.as_str()), "memory id not in {ids:?}");
}

#[tokio::test]
async fn search_kind_filter_scopes_to_one_domain() {
    let (addr, _state, client, _tr) = boot().await;
    let _task = create_task(&client, addr, "frobnicator throughput regression").await;
    let _mem = create_memory(&client, addr, "frobnicator wedges under load").await;

    // Filter to gotcha only — the task hit must drop out.
    let resp = client
        .get(format!(
            "http://{addr}/api/search?q=frobnicator&mode=lexical&kind=gotcha"
        ))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let hits = body["hits"].as_array().expect("hits");
    assert!(!hits.is_empty(), "expected at least one gotcha hit");
    for hit in hits {
        assert_eq!(hit["kind"], "gotcha", "kind filter leaked: {hit}");
    }
}

#[tokio::test]
async fn search_unknown_kind_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/search?q=x&kind=widget"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_limit_over_max_is_400() {
    let (addr, _state, client, _tr) = boot().await;
    let resp = client
        .get(format!("http://{addr}/api/search?q=x&limit=1000"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}
