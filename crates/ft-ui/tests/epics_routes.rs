//! Integration tests for the `/api/epics` axum surface (firetrail-6no5.10).
//!
//! Spins up an in-process server bound to an ephemeral port, bootstraps a
//! session cookie, and exercises the `GET /api/epics` route. Fixture/server
//! bootstrap mirrors `tests/tickets_routes.rs` faithfully.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use ft_testkit::TestRepo;
use ft_ui::server::{AppState, test_app};
use tokio::net::TcpListener;

const TEST_IDENTITY: &str = "alice@firetrail.test";

/// Build a workspace fixture with `.firetrail/config.yml` (strict=false)
/// and a static `.firetrail/identity.yml` so [`ft_identity::DefaultResolver`]
/// doesn't need to consult environment variables.
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

/// Spin up a server, bootstrap the session, and return `(addr, state, client, tr)`.
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

/// POST to `/api/tickets` with `kind=epic` and return the response body.
async fn create_epic(client: &reqwest::Client, addr: SocketAddr, title: &str) -> serde_json::Value {
    let resp = client
        .post(format!("http://{addr}/api/tickets"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "kind": "epic",
            "title": title,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "create_epic");
    resp.json().await.unwrap()
}

/// POST to `/api/tickets` with `kind=task` (attached to `epic_id`) and return the body.
async fn create_task_under_epic(
    client: &reqwest::Client,
    addr: SocketAddr,
    title: &str,
    epic_id: &str,
) -> serde_json::Value {
    let resp = client
        .post(format!("http://{addr}/api/tickets"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({
            "kind": "task",
            "title": title,
            "epic": epic_id,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::CREATED,
        "create_task_under_epic"
    );
    resp.json().await.unwrap()
}

/// POST to `/api/tickets/:id/close`.
async fn close_ticket(client: &reqwest::Client, addr: SocketAddr, id: &str) {
    let resp = client
        .post(format!("http://{addr}/api/tickets/{id}/close"))
        .header("Host", addr.to_string())
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "close_ticket {id}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn epics_returns_empty_on_fresh_workspace() {
    let (addr, _state, client, _tr) = boot().await;

    let resp = client
        .get(format!("http://{addr}/api/epics"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let epics = body.get("epics").and_then(|v| v.as_array()).expect("epics");
    assert!(epics.is_empty(), "expected no epics on fresh workspace");
}

#[tokio::test]
async fn epics_ready_to_close_when_all_children_closed() {
    let (addr, _state, client, _tr) = boot().await;

    // Create an epic.
    let epic_body = create_epic(&client, addr, "my-epic").await;
    let epic_id = epic_body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create a child task under the epic.
    let task_body = create_task_under_epic(&client, addr, "child-task", &epic_id).await;
    let task_id = task_body["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Before closing the child, ready_to_close should be false.
    let resp = client
        .get(format!("http://{addr}/api/epics"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let epics = body["epics"].as_array().expect("epics");
    assert_eq!(epics.len(), 1, "expected 1 epic");
    let epic = &epics[0];
    assert_eq!(epic["id"].as_str().unwrap(), epic_id);
    assert_eq!(epic["child_total"].as_u64().unwrap(), 1, "child_total");
    assert_eq!(epic["child_closed"].as_u64().unwrap(), 0, "child_closed");
    assert!(
        !epic["ready_to_close"].as_bool().unwrap(),
        "should not be ready before child closes"
    );

    // Close the child task.
    close_ticket(&client, addr, &task_id).await;

    // Now ready_to_close should be true.
    let resp = client
        .get(format!("http://{addr}/api/epics"))
        .header("Host", addr.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let epics = body["epics"].as_array().expect("epics");
    assert_eq!(epics.len(), 1, "expected 1 epic");
    let epic = &epics[0];
    assert_eq!(epic["child_total"].as_u64().unwrap(), 1, "child_total");
    assert_eq!(epic["child_closed"].as_u64().unwrap(), 1, "child_closed");
    assert!(
        epic["ready_to_close"].as_bool().unwrap(),
        "should be ready after child closes"
    );
}
