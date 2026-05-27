//! Claim / unclaim tests, including the concurrent-claim atomicity check.

mod common;

use std::path::Path;
use std::process::Command;

use common::{fresh_repo, id_from_create, run_firetrail};

#[test]
fn claim_creates_claim_with_expiry() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let out = run_firetrail(tr.root(), &["--json", "claim", &id, "--expires", "1h"]);
    assert!(out.success(), "claim failed: {}", out.stderr);
    let v = common::parse_json(&out);
    assert!(!v["data"]["record"]["body"]["claim"].is_null());
    assert!(
        v["data"]["record"]["body"]["claim"]["claim_expires_at"]
            .as_str()
            .is_some()
    );
}

#[test]
fn claim_then_second_claim_conflicts() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let first = run_firetrail(tr.root(), &["--json", "claim", &id]);
    assert!(first.success(), "first claim failed: {}", first.stderr);

    // Second claim from a different identity must fail with conflict.
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["--json", "claim", &id])
        .current_dir(tr.root())
        .env("FIRETRAIL_AUTHOR", "other@firetrail.test")
        .output()
        .unwrap();
    assert!(!out.status.success(), "second claim should fail");
    assert_eq!(out.status.code(), Some(3), "expected conflict exit code");
}

#[test]
fn unclaim_releases_claim() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["--json", "claim", &id]);
    let out = run_firetrail(tr.root(), &["--json", "unclaim", &id]);
    assert!(out.success(), "unclaim failed: {}", out.stderr);
    let v = common::parse_json(&out);
    assert!(v["data"]["record"]["body"]["claim"].is_null());
}

/// The headline acceptance criterion: two concurrent claims, exactly one wins.
#[test]
fn concurrent_claim_yields_exactly_one_winner() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let id1 = id.clone();
    let id2 = id.clone();
    let path1 = tr.root().to_path_buf();
    let path2 = tr.root().to_path_buf();

    let h1 = std::thread::spawn(move || spawn_claim(&path1, &id1, "alice@example.com"));
    let h2 = std::thread::spawn(move || spawn_claim(&path2, &id2, "bob@example.com"));
    let r1 = h1.join().unwrap();
    let r2 = h2.join().unwrap();

    let outcomes = [&r1, &r2];
    let successes: Vec<&&ClaimOutput> = outcomes.iter().filter(|o| o.status.success()).collect();
    let failures: Vec<&&ClaimOutput> = outcomes.iter().filter(|o| !o.status.success()).collect();
    assert_eq!(
        successes.len(),
        1,
        "expected exactly one success — r1.status={:?}, r2.status={:?}\n--- r1 stdout ---\n{}\n--- r1 stderr ---\n{}\n--- r2 stdout ---\n{}\n--- r2 stderr ---\n{}",
        r1.status,
        r2.status,
        r1.stdout,
        r1.stderr,
        r2.stdout,
        r2.stderr,
    );
    assert_eq!(failures.len(), 1, "expected exactly one failure");
    assert_eq!(
        failures[0].status.code(),
        Some(3),
        "failing claim must exit with conflict code"
    );
}

fn spawn_claim(root: &Path, id: &str, who: &str) -> ClaimOutput {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["--json", "claim", id])
        .current_dir(root)
        .env("FIRETRAIL_AUTHOR", who)
        .output()
        .unwrap();
    ClaimOutput {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[derive(Debug)]
struct ClaimOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}
