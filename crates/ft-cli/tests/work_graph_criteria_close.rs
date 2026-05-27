//! Acceptance criteria + close/reopen integration tests.

mod common;

use common::{fresh_repo, id_from_create, parse_json, run_firetrail};

#[test]
fn close_refuses_with_incomplete_criteria() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["--json", "criteria", "add", &id, "AC text"]);
    let out = run_firetrail(tr.root(), &["--json", "close", &id]);
    assert!(!out.success(), "close should refuse: {}", out.stdout);
    let v: serde_json::Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], 1);
    let incomplete = v["error"]["details"]["incomplete"]
        .as_array()
        .expect("incomplete details present");
    assert_eq!(incomplete.len(), 1);
}

#[test]
fn close_force_with_reason_succeeds() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["--json", "criteria", "add", &id, "AC text"]);
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "close",
            &id,
            "--force",
            "--reason",
            "shipped anyway",
        ],
    );
    assert!(out.success(), "force close should succeed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["status"], "closed");
    let labels = v["data"]["record"]["envelope"]["labels"]
        .as_array()
        .expect("labels array");
    assert!(
        labels
            .iter()
            .any(|l| l["key"] == "force_close_reason" && l["value"] == "shipped anyway"),
        "force reason recorded as a label"
    );
}

#[test]
fn close_succeeds_when_all_criteria_checked() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["--json", "criteria", "add", &id, "first ac"]);
    run_firetrail(tr.root(), &["--json", "criteria", "add", &id, "second ac"]);
    run_firetrail(tr.root(), &["--json", "criteria", "check", &id, "1"]);
    run_firetrail(tr.root(), &["--json", "criteria", "check", &id, "ac-02"]);
    let out = run_firetrail(tr.root(), &["--json", "close", &id]);
    assert!(out.success(), "close should succeed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["status"], "closed");
}

#[test]
fn criteria_check_uncheck_round_trip() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["--json", "criteria", "add", &id, "the AC"]);
    run_firetrail(tr.root(), &["--json", "criteria", "check", &id, "1"]);
    let out = run_firetrail(tr.root(), &["--json", "criteria", "list", &id]);
    let v = parse_json(&out);
    assert_eq!(v["data"]["items"][0]["checked"], true);

    run_firetrail(tr.root(), &["--json", "criteria", "uncheck", &id, "1"]);
    let out = run_firetrail(tr.root(), &["--json", "criteria", "list", &id]);
    let v = parse_json(&out);
    assert_eq!(v["data"]["items"][0]["checked"], false);
}

#[test]
fn criteria_evidence_persists_url() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["--json", "criteria", "add", &id, "the AC"]);
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "criteria",
            "evidence",
            &id,
            "1",
            "--url",
            "https://example.com/run/123",
        ],
    );
    assert!(out.success(), "evidence failed: {}", out.stderr);
    let out = run_firetrail(tr.root(), &["--json", "criteria", "list", &id]);
    let v = parse_json(&out);
    assert_eq!(
        v["data"]["items"][0]["evidence_url"],
        "https://example.com/run/123"
    );
}

#[test]
fn reopen_clears_closed_at() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(
        tr.root(),
        &["--json", "close", &id, "--force", "--reason", "force-close"],
    );
    let out = run_firetrail(tr.root(), &["--json", "reopen", &id]);
    assert!(out.success(), "reopen failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["status"], "open");
    assert!(v["data"]["record"]["envelope"]["closed_at"].is_null());
}
