//! list / ready / board / graph integration tests.

mod common;

use common::{fresh_repo, id_from_create, parse_json, run_firetrail};

#[test]
fn list_filters_by_kind_and_status() {
    let tr = fresh_repo();
    let _e = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "epic", "create", "the epic"],
    ));
    let t = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "the task"],
    ));
    run_firetrail(
        tr.root(),
        &["--json", "update", &t, "--status", "in_progress"],
    );
    let out = run_firetrail(tr.root(), &["--json", "list", "--type", "task"]);
    let v = parse_json(&out);
    let rows = v["data"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "in_progress");
}

#[test]
fn ready_returns_unblocked_open_records() {
    let tr = fresh_repo();
    let t = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "ready task"],
    ));
    run_firetrail(tr.root(), &["--json", "update", &t, "--status", "ready"]);
    let out = run_firetrail(tr.root(), &["--json", "ready"]);
    assert!(out.success(), "ready failed: {}", out.stderr);
    let v = parse_json(&out);
    let rows = v["data"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["id"] == t));
}

#[test]
fn board_groups_records_by_status() {
    let tr = fresh_repo();
    let a = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "A"],
    ));
    let b = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "B"],
    ));
    run_firetrail(tr.root(), &["--json", "update", &b, "--status", "review"]);
    let _ = a;
    let out = run_firetrail(tr.root(), &["--json", "board"]);
    let v = parse_json(&out);
    let todo = v["data"]["todo"].as_array().unwrap();
    let review = v["data"]["review"].as_array().unwrap();
    assert_eq!(todo.len(), 1);
    assert_eq!(review.len(), 1);
}

#[test]
fn graph_walks_parent_child_edges() {
    let tr = fresh_repo();
    let epic = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "epic", "create", "the epic"],
    ));
    let _t = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "child task", "--epic", &epic],
    ));
    let out = run_firetrail(tr.root(), &["--json", "graph", &epic]);
    assert!(out.success(), "graph failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["root"], epic);
    // Regression for firetrail-1sg: parent-of edges must be present, not empty.
    let edges = v["data"]["edges"].as_object().expect("edges is an object");
    assert!(!edges.is_empty(), "expected non-empty edges, got {edges:?}");
    let from_epic = edges
        .get(epic.as_str())
        .and_then(|v| v.as_array())
        .expect("edges keyed by epic root");
    assert!(from_epic.iter().any(|n| n["kind"] == "parent-of"));
}

#[test]
fn graph_empty_edges_carries_reason() {
    // Regression for firetrail-1sg: an empty edges map must be
    // self-describing so callers can distinguish "no relations" from a
    // query bug.
    let tr = fresh_repo();
    let lone = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "no relations"],
    ));
    let out = run_firetrail(tr.root(), &["--json", "graph", &lone]);
    assert!(out.success(), "graph failed: {}", out.stderr);
    let v = parse_json(&out);
    assert!(v["data"]["edges"].as_object().unwrap().is_empty());
    assert!(
        v["data"]["reason"].as_str().is_some(),
        "expected `reason` field when edges is empty"
    );
}

#[test]
fn board_markdown_is_stable() {
    let tr = fresh_repo();
    let _a = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "A"],
    ));
    let b = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "B"],
    ));
    run_firetrail(
        tr.root(),
        &["--json", "update", &b, "--status", "in_progress"],
    );
    let out = run_firetrail(tr.root(), &["--format", "markdown", "board"]);
    assert!(out.success(), "board markdown failed: {}", out.stderr);
    // The exact bytes vary by random ids — snapshot only the structural shape.
    insta::with_settings!({
        filters => vec![
            (r"TASK-[0-9a-f]{6,}", "TASK-<hash>"),
        ]
    }, {
        insta::assert_snapshot!("board_markdown", out.stdout);
    });
}

#[test]
fn graph_markdown_is_stable() {
    let tr = fresh_repo();
    let epic = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "epic", "create", "the epic"],
    ));
    let _t = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "child task", "--epic", &epic],
    ));
    let out = run_firetrail(tr.root(), &["--format", "markdown", "graph", &epic]);
    assert!(out.success(), "graph markdown failed: {}", out.stderr);
    insta::with_settings!({
        filters => vec![
            (r"(TASK|EPIC)-[0-9a-f]{6,}", "$1-<hash>"),
        ]
    }, {
        insta::assert_snapshot!("graph_markdown", out.stdout);
    });
}
