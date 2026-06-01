//! Integration tests for the `firetrail scope add/edit/rm/reorder` write
//! surface (scope-authoring epic Phase .2).
//!
//! Each test shells out to the built binary in a fresh repo and asserts the
//! resulting `.firetrail/scopes.yaml` is what `scope list` / `ScopeRegistry`
//! sees.

mod common;

use common::{fresh_repo, parse_json, run_firetrail};

/// `scope add` writes a scope that `scope list` (and the loader) can see.
#[test]
fn scope_add_writes_scope_visible_to_list() {
    let tr = fresh_repo();
    let added = run_firetrail(
        tr.root(),
        &[
            "--json",
            "scope",
            "add",
            "apps/checkout",
            "--applies-to",
            "apps/checkout/**",
            "--name",
            "Checkout",
            "--alias",
            "checkout",
        ],
    );
    assert!(added.success(), "add failed: {}", added.stderr);

    // The file now exists and is readable by `scope list`.
    assert!(
        tr.firetrail_dir().join("scopes.yaml").exists(),
        "scopes.yaml not written"
    );
    let listed = run_firetrail(tr.root(), &["--json", "scope", "list"]);
    assert!(listed.success(), "list failed: {}", listed.stderr);
    let v = parse_json(&listed);
    let scopes = v["data"]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0]["id"], "apps/checkout");
    assert_eq!(scopes[0]["name"], "Checkout");

    // And `scope show` resolves the alias.
    let show = run_firetrail(tr.root(), &["--json", "scope", "show", "checkout"]);
    assert!(show.success(), "show failed: {}", show.stderr);
    let v = parse_json(&show);
    assert_eq!(v["data"]["scope"]["id"], "apps/checkout");
}

/// Adding a duplicate id surfaces a user error.
#[test]
fn scope_add_duplicate_id_errors() {
    let tr = fresh_repo();
    let first = run_firetrail(
        tr.root(),
        &[
            "scope",
            "add",
            "apps/checkout",
            "--applies-to",
            "apps/checkout/**",
        ],
    );
    assert!(first.success(), "first add failed: {}", first.stderr);

    let dup = run_firetrail(
        tr.root(),
        &[
            "scope",
            "add",
            "apps/checkout",
            "--applies-to",
            "apps/other/**",
        ],
    );
    assert!(!dup.success(), "duplicate add should fail");
    assert!(
        dup.stderr.contains("duplicate") || dup.stderr.contains("apps/checkout"),
        "stderr: {}",
        dup.stderr
    );
}

/// `scope add` requires at least one `--applies-to`.
#[test]
fn scope_add_missing_applies_to_errors() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["scope", "add", "apps/checkout"]);
    assert!(!out.success(), "add without --applies-to should fail");
}

/// `scope edit` changes a single field, preserving the rest.
#[test]
fn scope_edit_changes_field() {
    let tr = fresh_repo();
    assert!(
        run_firetrail(
            tr.root(),
            &[
                "scope",
                "add",
                "apps/checkout",
                "--applies-to",
                "apps/checkout/**",
                "--name",
                "Checkout",
            ],
        )
        .success()
    );

    let edited = run_firetrail(
        tr.root(),
        &["--json", "scope", "edit", "apps/checkout", "--name", "Cart"],
    );
    assert!(edited.success(), "edit failed: {}", edited.stderr);

    let listed = run_firetrail(tr.root(), &["--json", "scope", "list"]);
    let v = parse_json(&listed);
    let scopes = v["data"]["scopes"].as_array().unwrap();
    assert_eq!(scopes[0]["name"], "Cart");
    // applies_to preserved (not cleared by editing only the name).
    assert_eq!(scopes[0]["applies_to"][0], "apps/checkout/**");
}

/// `scope edit` on an unknown id errors.
#[test]
fn scope_edit_absent_errors() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["scope", "edit", "nope", "--name", "X"]);
    assert!(!out.success(), "edit of absent scope should fail");
}

/// `scope rm` removes a scope; removing an absent one errors.
#[test]
fn scope_rm_removes_and_absent_errors() {
    let tr = fresh_repo();
    assert!(
        run_firetrail(
            tr.root(),
            &[
                "scope",
                "add",
                "apps/checkout",
                "--applies-to",
                "apps/checkout/**"
            ],
        )
        .success()
    );
    let removed = run_firetrail(tr.root(), &["--json", "scope", "rm", "apps/checkout"]);
    assert!(removed.success(), "rm failed: {}", removed.stderr);

    let listed = run_firetrail(tr.root(), &["--json", "scope", "list"]);
    let v = parse_json(&listed);
    assert_eq!(v["data"]["scopes"].as_array().unwrap().len(), 0);

    // Removing again errors.
    let absent = run_firetrail(tr.root(), &["scope", "rm", "apps/checkout"]);
    assert!(!absent.success(), "rm of absent scope should fail");
}

/// `scope reorder` changes declaration order.
#[test]
fn scope_reorder_changes_order() {
    let tr = fresh_repo();
    for id in ["a", "b", "c"] {
        assert!(
            run_firetrail(
                tr.root(),
                &["scope", "add", id, "--applies-to", &format!("{id}/**")],
            )
            .success(),
            "add {id} failed"
        );
    }
    // Declared order is a, b, c. Reorder to c, a, b.
    let reordered = run_firetrail(tr.root(), &["--json", "scope", "reorder", "c", "a", "b"]);
    assert!(reordered.success(), "reorder failed: {}", reordered.stderr);

    let listed = run_firetrail(tr.root(), &["--json", "scope", "list"]);
    let v = parse_json(&listed);
    let scopes = v["data"]["scopes"].as_array().unwrap();
    let ids: Vec<&str> = scopes.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["c", "a", "b"]);
}

/// Zero-overhead: a repo with no scopes.yaml and no scope-write command keeps
/// having no scopes.yaml — firetrail never auto-creates it.
#[test]
fn no_scopes_yaml_without_write_command() {
    let tr = fresh_repo();
    // A read-only scope command must not materialise the file.
    let listed = run_firetrail(tr.root(), &["--json", "scope", "list"]);
    assert!(listed.success(), "list failed: {}", listed.stderr);
    assert!(
        !tr.firetrail_dir().join("scopes.yaml").exists(),
        "scopes.yaml should not be auto-created"
    );
}
