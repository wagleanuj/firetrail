//! Integration tests for the `create` commands and `update` / `show`.

mod common;

use common::{fresh_repo, id_from_create, parse_json, run_firetrail};

#[test]
fn task_create_writes_record_and_returns_id() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo task", "--priority", "p1"],
    );
    let id = id_from_create(&out);
    assert!(id.starts_with("TASK-"), "id should be TASK-prefixed: {id}");

    let kind_dir = tr.firetrail_dir().join("records/task");
    let entries: Vec<_> = std::fs::read_dir(kind_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1, "exactly one task on disk");
}

#[test]
fn epic_create_then_task_create_with_parent() {
    let tr = fresh_repo();
    let epic = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "epic", "create", "demo epic"],
    ));
    let task = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo task", "--epic", &epic],
    ));
    assert!(task.starts_with("TASK-"));
}

#[test]
fn bug_create_with_service_and_severity() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "bug",
            "create",
            "demo bug",
            "--service",
            "auth",
            "--severity",
            "sev2",
        ],
    );
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["body"]["service"], "auth");
    assert_eq!(v["data"]["record"]["body"]["severity"], "sev2");
}

#[test]
fn subtask_requires_existing_parent() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "subtask",
            "create",
            "x",
            "--parent",
            "TASK-deadbeef",
        ],
    );
    assert!(!out.success(), "expected failure: {}", out.stdout);
    let v: serde_json::Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], 2, "expected not-found exit code 2");
}

#[test]
fn show_returns_full_record() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let out = run_firetrail(tr.root(), &["--json", "show", &id]);
    assert!(out.success(), "show failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["id"], id);
}

#[test]
fn show_accepts_unique_prefix() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let prefix = &id[..15]; // TASK- + 10 hex chars
    let out = run_firetrail(tr.root(), &["--json", "show", prefix]);
    assert!(out.success(), "prefix show failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["id"], id);
}

#[test]
fn update_changes_status_and_priority() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "update",
            &id,
            "--status",
            "in_progress",
            "--priority",
            "p0",
        ],
    );
    assert!(out.success(), "update failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["status"], "in_progress");
    assert_eq!(v["data"]["record"]["envelope"]["priority"], "p0");
}

#[test]
fn update_can_edit_description() {
    // firetrail-y7s: --description rewrites the body.description field
    // on work-graph kinds, and the change is captured by the history chain.
    let tr = fresh_repo();
    let create = run_firetrail(
        tr.root(),
        &[
            "--json",
            "task",
            "create",
            "describe me",
            "--description",
            "v1",
        ],
    );
    let id = id_from_create(&create);
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "update",
            &id,
            "--description",
            "v2 - refined",
        ],
    );
    assert!(out.success(), "update --description failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["body"]["description"], "v2 - refined");
    let history = v["data"]["record"]["envelope"]["history"]
        .as_array()
        .expect("history is an array");
    assert!(
        history.len() >= 2,
        "expected history to grow (create + update), got {}",
        history.len()
    );
}

#[test]
fn update_requires_at_least_one_field() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let out = run_firetrail(tr.root(), &["--json", "update", &id]);
    assert!(!out.success(), "expected user error");
    let v: serde_json::Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], 1);
}
