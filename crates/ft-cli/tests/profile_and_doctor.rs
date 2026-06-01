//! `firetrail profile {show,set,component}` + `firetrail doctor` profile tiers
//! (firetrail-lj41.3 / .4).

mod common;

use common::{fresh_repo, parse_json, run_firetrail};
use ft_testkit::CmdOutput;

/// Pull the profile body out of a successful `profile` JSON envelope.
fn profile_body(out: &CmdOutput) -> serde_json::Value {
    assert!(out.success(), "profile command failed: {}", out.stderr);
    let v = parse_json(out);
    v["data"]["record"]["body"].clone()
}

/// Find a doctor check row by id.
fn doctor_check<'a>(v: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
    v["data"]["checks"]
        .as_array()?
        .iter()
        .find(|c| c["id"] == id)
}

/// Write a `.firetrail/scopes.yaml` declaring the given `(id, glob)` scopes.
fn write_scopes(root: &std::path::Path, scopes: &[(&str, &str)]) {
    use std::fmt::Write as _;
    let mut yaml = String::from("scopes:\n");
    for (id, glob) in scopes {
        let _ = write!(yaml, "  - id: {id}\n    applies_to: [\"{glob}\"]\n");
    }
    std::fs::write(root.join(".firetrail/scopes.yaml"), yaml).unwrap();
}

#[test]
fn profile_set_creates_then_partial_update_in_place() {
    let tr = fresh_repo();

    // First set: only validate + one language.
    let first = run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--validate",
            "cargo test",
            "--language",
            "rust",
        ],
    );
    let body = profile_body(&first);
    assert_eq!(body["kind"], "repo_profile");
    assert_eq!(body["validate_command"], "cargo test");
    assert_eq!(body["languages"], serde_json::json!(["rust"]));
    assert_eq!(body["trust"], "draft");
    let id_first = parse_json(&first)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Second set: only the test command. validate + languages must persist.
    let second = run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--test", "cargo nextest run"],
    );
    let body2 = profile_body(&second);
    assert_eq!(
        body2["validate_command"], "cargo test",
        "validate preserved"
    );
    assert_eq!(
        body2["languages"],
        serde_json::json!(["rust"]),
        "langs preserved"
    );
    assert_eq!(body2["test_command"], "cargo nextest run", "test set");
    let id_second = parse_json(&second)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Singleton: same record id across both sets.
    assert_eq!(
        id_first, id_second,
        "profile updated in place, not duplicated"
    );

    // Exactly one record file on disk.
    let dir = tr.root().join(".firetrail/records/repo_profile");
    let count = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .ok()
                .and_then(|e| e.path().extension().map(|x| x == "json"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 1, "exactly one profile file");
}

#[test]
fn profile_set_repeatable_vec_overwrites_only_when_present() {
    let tr = fresh_repo();
    run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--language",
            "rust",
            "--language",
            "typescript",
            "--package-manager",
            "cargo",
        ],
    );
    // Update an unrelated field — languages/package_managers must persist.
    let out = run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--runtime", "node 20"],
    );
    let body = profile_body(&out);
    assert_eq!(body["languages"], serde_json::json!(["rust", "typescript"]));
    assert_eq!(body["package_managers"], serde_json::json!(["cargo"]));
    assert_eq!(body["runtime"], "node 20");
}

#[test]
fn profile_show_json_shape_and_absent_errors_nonzero() {
    let tr = fresh_repo();

    // Absent: show exits non-zero (NotFound = exit 2).
    let absent = run_firetrail(tr.root(), &["--json", "profile", "show"]);
    assert!(!absent.success(), "show should fail when no profile exists");

    // Create, then show returns the full record body.
    run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--validate", "just ci"],
    );
    let shown = run_firetrail(tr.root(), &["--json", "profile", "show"]);
    let body = profile_body(&shown);
    assert_eq!(body["kind"], "repo_profile");
    assert_eq!(body["validate_command"], "just ci");
}

#[test]
fn profile_component_add_then_rm() {
    let tr = fresh_repo();
    let added = run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "component",
            "add",
            "ft-cli",
            "crates/ft-cli",
            "--summary",
            "the CLI",
        ],
    );
    let body = profile_body(&added);
    let comps = body["components"].as_array().unwrap();
    assert_eq!(comps.len(), 1);
    assert_eq!(comps[0]["name"], "ft-cli");
    assert_eq!(comps[0]["path"], "crates/ft-cli");
    assert_eq!(comps[0]["summary"], "the CLI");

    // rm removes it.
    let removed = run_firetrail(
        tr.root(),
        &["--json", "profile", "component", "rm", "ft-cli"],
    );
    let body2 = profile_body(&removed);
    assert!(body2["components"].as_array().is_none_or(Vec::is_empty));

    // rm of an unknown component errors.
    let missing = run_firetrail(tr.root(), &["--json", "profile", "component", "rm", "nope"]);
    assert!(!missing.success(), "rm of unknown component should fail");
}

#[test]
fn profile_set_scope_writes_owning_scope_only_when_scope_exists() {
    let tr = fresh_repo();
    write_scopes(tr.root(), &[("apps/checkout", "apps/checkout/**")]);

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--scope",
            "apps/checkout",
            "--test",
            "pnpm test",
        ],
    );
    let v = parse_json(&out);
    assert!(out.success(), "scoped set should succeed: {}", out.stderr);
    assert_eq!(
        v["data"]["record"]["envelope"]["owning_scope"], "apps/checkout",
        "owning_scope stamped"
    );
    let body = profile_body(&out);
    assert_eq!(body["test_command"], "pnpm test");
}

#[test]
fn profile_set_unknown_scope_errors() {
    let tr = fresh_repo();
    write_scopes(tr.root(), &[("apps/checkout", "apps/checkout/**")]);

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--scope",
            "nope",
            "--test",
            "pnpm test",
        ],
    );
    assert!(!out.success(), "unknown scope must error");
}

#[test]
fn profile_show_scope_and_resolved() {
    let tr = fresh_repo();
    write_scopes(tr.root(), &[("apps/checkout", "apps/checkout/**")]);

    // Base profile with validate + test.
    run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--validate",
            "just ci",
            "--test",
            "cargo test",
        ],
    );
    // Scope delta overrides only test.
    run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--scope",
            "apps/checkout",
            "--test",
            "pnpm test",
        ],
    );

    // show --scope prints the stored delta (test only; validate absent).
    let delta = run_firetrail(
        tr.root(),
        &["--json", "profile", "show", "--scope", "apps/checkout"],
    );
    let dbody = profile_body(&delta);
    assert_eq!(dbody["test_command"], "pnpm test");
    assert!(
        dbody["validate_command"].is_null(),
        "delta does not carry base validate"
    );

    // show --resolved --scope merges base ⊕ delta.
    let resolved = run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "show",
            "--scope",
            "apps/checkout",
            "--resolved",
        ],
    );
    let rbody = profile_body(&resolved);
    assert_eq!(rbody["validate_command"], "just ci", "inherited from base");
    assert_eq!(rbody["test_command"], "pnpm test", "overridden by delta");
}

#[test]
fn profile_no_scopes_yaml_zero_overhead() {
    // A repo with NO scopes.yaml and no --scope behaves exactly as before.
    let tr = fresh_repo();
    let set = run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--validate", "just ci"],
    );
    let sbody = profile_body(&set);
    assert_eq!(sbody["validate_command"], "just ci");
    assert!(
        parse_json(&set)["data"]["record"]["envelope"]["owning_scope"].is_null(),
        "base record has no owning_scope"
    );

    let show = run_firetrail(tr.root(), &["--json", "profile", "show"]);
    let shown = profile_body(&show);
    assert_eq!(shown["validate_command"], "just ci");
}

#[test]
fn profile_list_shows_base_and_scopes() {
    let tr = fresh_repo();
    write_scopes(
        tr.root(),
        &[
            ("apps/checkout", "apps/checkout/**"),
            ("libs/ui", "libs/ui/**"),
        ],
    );
    // Base + two scope deltas.
    run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--validate", "just ci"],
    );
    run_firetrail(
        tr.root(),
        &[
            "--json",
            "profile",
            "set",
            "--scope",
            "apps/checkout",
            "--validate",
            "pnpm test",
        ],
    );
    run_firetrail(
        tr.root(),
        &[
            "--json", "profile", "set", "--scope", "libs/ui", "--test", "vitest",
        ],
    );

    let out = run_firetrail(tr.root(), &["--json", "profile", "list"]);
    assert!(out.success(), "list failed: {}", out.stderr);
    let v = parse_json(&out);
    let rows = v["data"]["profiles"].as_array().expect("profiles array");
    assert_eq!(rows.len(), 3, "base + 2 scopes");

    let base = rows
        .iter()
        .find(|r| r["scope"] == "(base)")
        .expect("base row");
    assert_eq!(base["has_validate"], true);

    let checkout = rows
        .iter()
        .find(|r| r["scope"] == "apps/checkout")
        .expect("checkout row");
    assert_eq!(checkout["has_validate"], true);

    let ui = rows
        .iter()
        .find(|r| r["scope"] == "libs/ui")
        .expect("ui row");
    assert_eq!(ui["has_validate"], false, "ui set only test, not validate");
}

#[test]
fn doctor_warns_when_no_profile() {
    let tr = fresh_repo();
    let doc = run_firetrail(tr.root(), &["--json", "doctor"]);
    assert!(doc.success(), "doctor (non-strict) should not block");
    let v = parse_json(&doc);
    let present = doctor_check(&v, "profile.present").expect("profile.present check");
    assert_eq!(present["status"], "warn");
}

#[test]
fn doctor_warns_when_profile_has_no_validate() {
    let tr = fresh_repo();
    // Profile exists but with no validate command.
    run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--language", "rust"],
    );
    let v = parse_json(&run_firetrail(tr.root(), &["--json", "doctor"]));
    let validate = doctor_check(&v, "profile.validate").expect("profile.validate check");
    assert_eq!(validate["status"], "warn");
    // And it is still unconfirmed (Draft).
    let trust = doctor_check(&v, "profile.trust").expect("profile.trust check");
    assert_eq!(trust["status"], "warn");
}

#[test]
fn doctor_strict_fails_without_profile() {
    let tr = fresh_repo();
    let doc = run_firetrail(tr.root(), &["--json", "doctor", "--strict"]);
    assert!(!doc.success(), "--strict must fail with no profile");
    assert_eq!(doc.status.code(), Some(1), "user-error exit code");
}

#[test]
fn doctor_strict_fails_without_validate_or_unconfirmed() {
    let tr = fresh_repo();
    run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--language", "rust"],
    );
    // No validate command AND Draft → strict fails.
    let doc = run_firetrail(tr.root(), &["--json", "doctor", "--strict"]);
    assert!(
        !doc.success(),
        "--strict must fail without validate / unconfirmed"
    );
}

#[test]
fn doctor_strict_fails_when_validate_set_but_still_draft() {
    let tr = fresh_repo();
    run_firetrail(
        tr.root(),
        &["--json", "profile", "set", "--validate", "cargo test"],
    );
    // validate is set, but the profile is still Draft → strict still fails.
    let doc = run_firetrail(tr.root(), &["--json", "doctor", "--strict"]);
    assert!(
        !doc.success(),
        "--strict must fail while profile is unconfirmed"
    );
}
