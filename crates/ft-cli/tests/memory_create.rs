//! Memory-record `*-create` smoke tests + capture.

mod common;

use common::{fresh_repo, parse_json, run_firetrail};

#[test]
fn incident_create_writes_record_with_history() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "incident",
            "create",
            "redis pool exhausted",
            "--severity",
            "sev2",
            "--services",
            "checkout,api",
            "--risk-class",
            "availability",
        ],
    );
    assert!(out.success(), "incident create failed: {}", out.stderr);
    let v = parse_json(&out);
    let env = &v["data"]["record"]["envelope"];
    assert_eq!(env["kind"].as_str(), Some("incident"));
    let body = &v["data"]["record"]["body"];
    assert_eq!(body["kind"], "incident");
    assert_eq!(body["summary"], "redis pool exhausted");
    assert_eq!(body["severity"], "sev2");
    assert_eq!(body["risk_class"], "availability");
    // History bootstrapped with a Create entry.
    assert_eq!(env["history"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["state_hash"].as_str().unwrap().len(), 64);
}

#[test]
fn finding_create_with_parent_incident() {
    let tr = fresh_repo();
    let inc = parse_json(&run_firetrail(
        tr.root(),
        &["--json", "incident", "create", "x"],
    ))["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "finding",
            "create",
            "checkout latency tied to redis",
            "--incident",
            &inc,
            "--details",
            "we saw latency spike when redis pool maxed",
        ],
    );
    assert!(out.success(), "finding create failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["kind"], "finding");
}

#[test]
fn runbook_create_then_step_add() {
    let tr = fresh_repo();
    let id = parse_json(&run_firetrail(
        tr.root(),
        &[
            "--json",
            "runbook",
            "create",
            "drain redis pool",
            "--summary",
            "what to do when pool maxes",
        ],
    ))["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "runbook",
            "step",
            "add",
            &id,
            "--description",
            "flush stuck connections",
            "--command",
            "redis-cli CLIENT KILL TYPE normal",
            "--expected",
            "pool returns to baseline within 30s",
        ],
    );
    assert!(out.success(), "step add failed: {}", out.stderr);
    let v = parse_json(&out);
    let steps = &v["data"]["record"]["body"]["steps"];
    assert_eq!(steps.as_array().map(Vec::len), Some(1));
}

#[test]
fn decision_create_carries_fields() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "decision",
            "create",
            "use ONNX runtime",
            "--context",
            "we want offline embeddings",
            "--decision",
            "ship ONNX",
            "--consequences",
            "no GPU dep",
        ],
    );
    assert!(out.success(), "decision create failed: {}", out.stderr);
    let v = parse_json(&out);
    let body = &v["data"]["record"]["body"];
    assert_eq!(body["context"], "we want offline embeddings");
    assert_eq!(body["decision"], "ship ONNX");
}

#[test]
fn gotcha_create_and_memory_create() {
    let tr = fresh_repo();
    let g = run_firetrail(
        tr.root(),
        &[
            "--json",
            "gotcha",
            "create",
            "rusqlite bundled requires sqlite3 dev",
        ],
    );
    assert!(g.success(), "gotcha create failed: {}", g.stderr);

    let m = run_firetrail(
        tr.root(),
        &[
            "--json",
            "memory",
            "create",
            "weekly review",
            "--body",
            "we shipped M2 trust state machine.",
            "--tags",
            "review,m2",
        ],
    );
    assert!(m.success(), "memory create failed: {}", m.stderr);
    let v = parse_json(&m);
    let tags = v["data"]["record"]["body"]["tags"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(tags.len(), 2);
}

#[test]
fn capture_with_body_arg_creates_memory() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "capture",
            "--title",
            "noticed something",
            "--body",
            "ports above 32k drift on macOS",
        ],
    );
    assert!(out.success(), "capture failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["envelope"]["kind"], "memory");
}
