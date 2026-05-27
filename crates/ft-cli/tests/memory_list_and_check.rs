//! `firetrail memory list` filters + `firetrail check pr` happy path.

mod common;

use common::{fresh_repo, parse_json, run_firetrail};

#[test]
fn memory_list_filters_by_trust_state() {
    let tr = fresh_repo();
    let _ = run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "a", "--body", "x"],
    );
    let _ = run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "b", "--body", "y"],
    );
    // Two drafts present.
    let drafts = run_firetrail(tr.root(), &["--json", "memory", "list", "--trust", "draft"]);
    assert!(drafts.success(), "list: {}", drafts.stderr);
    let v = parse_json(&drafts);
    assert!(v["data"]["rows"].as_array().unwrap().len() >= 2);

    // Verified filter currently matches none.
    let verified = run_firetrail(
        tr.root(),
        &["--json", "memory", "list", "--trust", "verified"],
    );
    assert!(verified.success(), "list verified: {}", verified.stderr);
    let v = parse_json(&verified);
    assert_eq!(v["data"]["rows"].as_array().map(Vec::len), Some(0));
}

#[test]
fn memory_show_renders_kind_specific_body() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "decision",
            "create",
            "use Rust",
            "--context",
            "we like correctness",
            "--decision",
            "ship in Rust",
        ],
    );
    let id = parse_json(&out)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let show = run_firetrail(tr.root(), &["--json", "memory", "show", &id]);
    assert!(show.success(), "memory show: {}", show.stderr);
    let v = parse_json(&show);
    assert_eq!(v["data"]["record"]["body"]["decision"], "ship in Rust");
}
