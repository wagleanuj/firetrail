//! Link / dep / show tests.

mod common;

use common::{fresh_repo, id_from_create, parse_json, run_firetrail};

#[test]
fn link_writes_relation_visible_to_show() {
    let tr = fresh_repo();
    let a = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "task A"],
    ));
    let b = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "task B"],
    ));
    let out = run_firetrail(
        tr.root(),
        &["--json", "link", &a, &b, "--type", "related-to"],
    );
    assert!(out.success(), "link failed: {}", out.stderr);

    let out = run_firetrail(tr.root(), &["--json", "show", &a]);
    let v = parse_json(&out);
    let relations = v["data"]["relations"].as_array().expect("relations array");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0]["kind"], "related-to");
}

#[test]
fn dep_add_then_remove() {
    let tr = fresh_repo();
    let a = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "task A"],
    ));
    let b = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "task B"],
    ));
    run_firetrail(
        tr.root(),
        &["--json", "dep", "add", &a, &b, "--type", "blocked-by"],
    );
    let out = run_firetrail(tr.root(), &["--json", "dep", "remove", &a, &b]);
    assert!(out.success(), "dep remove failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["removed"], 1);
}

#[test]
fn link_refuses_self_edge() {
    let tr = fresh_repo();
    let a = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "task A"],
    ));
    let out = run_firetrail(
        tr.root(),
        &["--json", "link", &a, &a, "--type", "related-to"],
    );
    assert!(!out.success(), "self-link should fail");
    let v: serde_json::Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], 1);
}
