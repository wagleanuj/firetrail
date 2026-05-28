//! Regression tests for two P1 bugs found during hands-on test drive:
//!
//! - firetrail-8ql: strict-identity gate not enforced for writes.
//! - firetrail-65q: history chain not populated on update / write commands.
//!
//! Both bugs share the same choke point (`WorkCtx::save_record` / `actor`),
//! so the fixes ship together and are covered side-by-side here.

mod common;

use std::path::Path;
use std::process::Command;

use common::{fresh_repo, parse_json, run_firetrail};
use ft_testkit::{CmdOutput, TestRepo};

/// Like `common::run_firetrail` but lets us override `FIRETRAIL_AUTHOR`
/// for the spawned process so we can simulate "unregistered actor".
fn run_firetrail_as(root: &Path, actor: &str, args: &[&str]) -> CmdOutput {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(root);
    cmd.env("FIRETRAIL_AUTHOR", actor);
    let output = cmd.output().expect("spawn firetrail");
    CmdOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status,
    }
}

/// Bootstrap a fresh repo initialised with `--strict-identity` and a
/// single registered identity for `alice@example.com`.
fn fresh_strict_repo_with_alice() -> TestRepo {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let init = run_firetrail(
        tr.root(),
        &[
            "init",
            "--json",
            "--strict-identity",
            "--no-hooks",
            "--no-agents",
        ],
    );
    assert!(init.success(), "init failed: {}", init.stderr);
    let reg = run_firetrail(
        tr.root(),
        &[
            "--json",
            "identity",
            "register",
            "alice",
            "--name",
            "Alice",
            "--emails",
            "alice@example.com",
            "--kind",
            "human",
        ],
    );
    assert!(reg.success(), "register failed: {}", reg.stderr);
    tr
}

// ── firetrail-8ql: strict-identity enforcement ────────────────────────────

#[test]
fn strict_identity_rejects_unregistered_actor_on_create() {
    let tr = fresh_strict_repo_with_alice();
    // Eve is NOT in the registry. Strict mode must reject her write.
    let out = run_firetrail_as(
        tr.root(),
        "eve@unknown.dev",
        &["--json", "task", "create", "sneaky"],
    );
    assert!(
        !out.success(),
        "expected strict-identity rejection, but task create succeeded: stdout={} stderr={}",
        out.stdout,
        out.stderr,
    );
    // The error envelope is emitted to stderr (since the success-path JSON
    // goes to stdout). The message must call out registration.
    let stderr_joined = format!("{}{}", out.stdout, out.stderr);
    assert!(
        stderr_joined.contains("is not registered"),
        "expected `is not registered` in error output, got stderr={} stdout={}",
        out.stderr,
        out.stdout,
    );
}

#[test]
fn strict_identity_allows_registered_actor() {
    let tr = fresh_strict_repo_with_alice();
    let out = run_firetrail_as(
        tr.root(),
        "alice@example.com",
        &["--json", "task", "create", "alice's task"],
    );
    assert!(
        out.success(),
        "registered alice should be allowed; stderr={}",
        out.stderr,
    );
}

#[test]
fn strict_identity_rejects_unregistered_on_update() {
    let tr = fresh_strict_repo_with_alice();
    // Alice creates a task; Eve tries to update it.
    let create = run_firetrail_as(
        tr.root(),
        "alice@example.com",
        &["--json", "task", "create", "alice's task"],
    );
    assert!(create.success(), "alice create failed: {}", create.stderr);
    let id = parse_json(&create)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = run_firetrail_as(
        tr.root(),
        "eve@unknown.dev",
        &["--json", "update", &id, "--title", "hijacked"],
    );
    assert!(
        !out.success(),
        "expected strict-identity rejection on update; stdout={} stderr={}",
        out.stdout,
        out.stderr,
    );
    let combined = format!("{}{}", out.stdout, out.stderr);
    assert!(
        combined.contains("is not registered"),
        "expected `is not registered` in error, got: {combined}",
    );
}

#[test]
fn non_strict_mode_allows_unregistered_actor() {
    // Sanity: when strict_identity is false, the M1 resolver still works
    // for unregistered identities.
    let tr = fresh_repo();
    let out = run_firetrail_as(
        tr.root(),
        "eve@unknown.dev",
        &["--json", "task", "create", "open repo"],
    );
    assert!(
        out.success(),
        "non-strict repo should allow any valid identity; stderr={}",
        out.stderr,
    );
}

// ── firetrail-65q: history chain populated on every write ─────────────────

#[test]
fn task_create_seeds_history_with_create_entry() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["--json", "task", "create", "h"]);
    assert!(out.success(), "create failed: {}", out.stderr);
    let v = parse_json(&out);
    let env = &v["data"]["record"]["envelope"];
    let history = env["history"].as_array().expect("history present");
    assert_eq!(
        history.len(),
        1,
        "create should seed exactly one history entry; got {history:?}",
    );
    let entry = &history[0];
    let first_op = entry["ops_summary"][0].as_str().unwrap();
    assert!(
        first_op.starts_with("create:"),
        "first history entry must carry the create tag; got `{first_op}`",
    );
    assert_eq!(
        entry["from_hash"].as_str(),
        Some(""),
        "genesis entry must have empty from_hash",
    );
    // prev_state_hash is None on genesis (serialised as `null` or omitted).
    assert!(
        env["prev_state_hash"].is_null(),
        "prev_state_hash must be null on genesis; got {}",
        env["prev_state_hash"],
    );
}

#[test]
fn update_chains_prev_state_hash_and_appends_history() {
    let tr = fresh_repo();
    let create = run_firetrail(tr.root(), &["--json", "task", "create", "h"]);
    assert!(create.success(), "create failed: {}", create.stderr);
    let created = parse_json(&create);
    let id = created["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let hash_after_create = created["data"]["record"]["envelope"]["state_hash"]
        .as_str()
        .unwrap()
        .to_string();

    // First update.
    let up1 = run_firetrail(tr.root(), &["--json", "update", &id, "--title", "v2"]);
    assert!(up1.success(), "update 1 failed: {}", up1.stderr);
    let v1 = parse_json(&up1);
    let env1 = &v1["data"]["record"]["envelope"];
    let history1 = env1["history"].as_array().expect("history present");
    assert_eq!(
        history1.len(),
        2,
        "first update should grow history to 2 entries; got {history1:?}",
    );
    // prev_state_hash must now point at the create entry's to_hash.
    let create_to_hash = history1[0]["to_hash"].as_str().unwrap().to_string();
    assert_eq!(
        env1["prev_state_hash"].as_str(),
        Some(create_to_hash.as_str()),
        "prev_state_hash must equal history[0].to_hash after first update",
    );
    assert_eq!(
        history1[1]["from_hash"].as_str(),
        Some(create_to_hash.as_str())
    );
    // state_hash must change relative to the create hash.
    assert_ne!(env1["state_hash"].as_str().unwrap(), hash_after_create);

    // Second update.
    let up2 = run_firetrail(tr.root(), &["--json", "update", &id, "--priority", "p1"]);
    assert!(up2.success(), "update 2 failed: {}", up2.stderr);
    let v2 = parse_json(&up2);
    let env2 = &v2["data"]["record"]["envelope"];
    let history2 = env2["history"].as_array().expect("history present");
    assert_eq!(history2.len(), 3, "second update should grow history to 3");
    // Chain integrity: each entry's from_hash equals prior to_hash.
    for i in 1..history2.len() {
        let prior_to = history2[i - 1]["to_hash"].as_str().unwrap();
        let this_from = history2[i]["from_hash"].as_str().unwrap();
        assert_eq!(
            this_from,
            prior_to,
            "history[{i}].from_hash must equal history[{}].to_hash; broken chain",
            i - 1,
        );
    }
}

#[test]
fn verify_after_updates_reports_zero_failures() {
    // End-to-end: create + multiple updates, then `firetrail verify` must
    // observe an intact chain and report no failures.
    let tr = fresh_repo();
    let create = run_firetrail(tr.root(), &["--json", "task", "create", "h"]);
    assert!(create.success(), "create failed: {}", create.stderr);
    let id = parse_json(&create)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let up1 = run_firetrail(tr.root(), &["--json", "update", &id, "--title", "v2"]);
    assert!(up1.success(), "update 1 failed: {}", up1.stderr);
    let up2 = run_firetrail(tr.root(), &["--json", "update", &id, "--priority", "p1"]);
    assert!(up2.success(), "update 2 failed: {}", up2.stderr);

    let verify = run_firetrail(tr.root(), &["--json", "verify", "--all"]);
    assert!(verify.success(), "verify failed: {}", verify.stderr);
    let v = parse_json(&verify);
    assert_eq!(
        v["data"]["failures"].as_u64(),
        Some(0),
        "verify should report 0 failures with intact chain; payload={v}",
    );
}
