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
