//! Integration tests for `firetrail init` and `firetrail doctor`.
//!
//! Each test spawns the real binary against a tempdir-backed `TestRepo` from
//! `ft-testkit`. We invoke the binary directly using `env!("CARGO_BIN_EXE_firetrail")`
//! rather than `TestRepo::firetrail()` because `ft-testkit` reads the env var
//! with `option_env!` at *its* compile time — ft-testkit has no build-time
//! dep on ft-cli (and shouldn't, to avoid a cycle), so the path is only
//! available when the test target is ft-cli's own integration suite.

use std::path::Path;
use std::process::Command;

use ft_testkit::{CmdOutput, TestRepo};

fn run_firetrail(root: &Path, args: &[&str]) -> CmdOutput {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let output = Command::new(bin)
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn firetrail");
    CmdOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status,
    }
}

#[test]
fn help_lists_init_and_doctor() {
    let tr = TestRepo::new().unwrap();
    let out = run_firetrail(tr.root(), &["--help"]);
    assert!(out.success(), "--help should exit 0: stderr={}", out.stderr);
    assert!(
        out.stdout.contains("init"),
        "help text missing init: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("doctor"),
        "help text missing doctor: {}",
        out.stdout
    );
}

#[test]
fn init_produces_valid_workspace() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();

    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: stderr={}", out.stderr);

    let v: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("init --json should be parseable");
    assert_eq!(v["format_version"], 1);
    assert_eq!(v["command"], "init");
    let fresh = v["data"]["fresh"].as_bool().expect("data.fresh present");
    assert!(fresh);

    assert!(tr.firetrail_dir().exists());
    assert!(tr.firetrail_dir().join("config.yml").exists());
    assert!(tr.firetrail_dir().join("identity.yml").exists());
    assert!(tr.firetrail_dir().join("index.db").exists());
    assert!(tr.firetrail_dir().join("records/task").exists());

    let hooks = tr.root().join(".git/hooks");
    assert!(hooks.join("pre-commit").exists(), "pre-commit installed");
    assert!(
        hooks.join("post-checkout").exists(),
        "post-checkout installed"
    );
    assert!(hooks.join("post-merge").exists(), "post-merge installed");

    let gitignore = std::fs::read_to_string(tr.root().join(".gitignore")).unwrap();
    assert!(
        gitignore.contains(".firetrail/index.db"),
        ".gitignore missing index.db entry: {gitignore}",
    );
}

#[test]
fn init_is_idempotent() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();

    let first = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(first.success(), "first init failed: {}", first.stderr);

    std::fs::write(
        tr.firetrail_dir().join("config.yml"),
        "format_version: 1\nuser_edit: true\n",
    )
    .unwrap();

    let second = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(second.success(), "second init failed: {}", second.stderr);
    let v: serde_json::Value = serde_json::from_str(&second.stdout).unwrap();
    assert_eq!(v["data"]["fresh"], false);

    let preserved = v["data"]["preserved"]
        .as_array()
        .expect("preserved is an array");
    assert!(
        preserved
            .iter()
            .any(|p| p.as_str() == Some(".firetrail/config.yml")),
        "config.yml should be preserved: {preserved:?}",
    );

    let cfg = std::fs::read_to_string(tr.firetrail_dir().join("config.yml")).unwrap();
    assert!(
        cfg.contains("user_edit: true"),
        "user edit not preserved across re-init: {cfg}",
    );
}

#[test]
fn doctor_clean_on_fresh_init() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let init = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(init.success(), "init failed: {}", init.stderr);

    let doc = run_firetrail(tr.root(), &["doctor", "--json"]);
    assert!(doc.success(), "doctor failed: stderr={}", doc.stderr);

    let v: serde_json::Value = serde_json::from_str(&doc.stdout).unwrap();
    assert_eq!(v["command"], "doctor");
    assert_eq!(v["data"]["clean"], true, "expected clean run: {v}");

    let checks = v["data"]["checks"].as_array().expect("checks array");
    assert!(!checks.is_empty());
    for c in checks {
        assert_eq!(
            c["status"],
            "ok",
            "check {} not ok: {c}",
            c["id"].as_str().unwrap_or("?")
        );
    }
}

#[test]
fn doctor_reports_missing_index_and_suggests_rebuild() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    run_firetrail(tr.root(), &["init"]);
    std::fs::remove_file(tr.firetrail_dir().join("index.db")).unwrap();

    let doc = run_firetrail(tr.root(), &["doctor", "--json"]);
    assert!(doc.success(), "doctor exited non-zero: {}", doc.stderr);
    let v: serde_json::Value = serde_json::from_str(&doc.stdout).unwrap();
    assert_eq!(v["data"]["clean"], false);

    let checks = v["data"]["checks"].as_array().expect("checks array");
    let integ = checks
        .iter()
        .find(|c| c["id"] == "index.integrity")
        .expect("index.integrity check present");
    assert_eq!(integ["status"], "fail", "expected FAIL: {integ}");
    let sg = integ["suggestion"].as_str().unwrap_or("");
    assert!(
        sg.contains("rebuild"),
        "suggestion should mention rebuild: {sg}"
    );
}

#[test]
fn doctor_fix_rebuilds_missing_index() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    run_firetrail(tr.root(), &["init"]);
    let db = tr.firetrail_dir().join("index.db");
    std::fs::remove_file(&db).unwrap();
    assert!(!db.exists());

    let doc = run_firetrail(tr.root(), &["doctor", "--fix", "--json"]);
    assert!(doc.success(), "doctor --fix failed: {}", doc.stderr);
    assert!(db.exists(), "index.db should be rebuilt by --fix");

    let v: serde_json::Value = serde_json::from_str(&doc.stdout).unwrap();
    let checks = v["data"]["checks"].as_array().unwrap();
    let integ = checks
        .iter()
        .find(|c| c["id"] == "index.integrity")
        .expect("index.integrity check present");
    assert_eq!(integ["status"], "ok");
    assert_eq!(integ["fix_applied"], true);
}

#[test]
fn invalid_subcommand_yields_clap_user_error() {
    let tr = TestRepo::new().unwrap();
    let out = run_firetrail(tr.root(), &["bogus"]);
    assert!(!out.success(), "expected non-zero exit for bogus command");
}

#[test]
fn outside_git_repo_returns_user_error_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_firetrail(dir.path(), &["--json", "doctor"]);
    assert!(!out.success(), "expected non-zero exit");
    assert_eq!(
        out.status.code().unwrap_or(-1),
        1,
        "expected exit code 1 (user error)"
    );
    let v: serde_json::Value =
        serde_json::from_str(&out.stderr).expect("error JSON should parse from stderr");
    assert_eq!(v["error"]["code"], 1);
    assert_eq!(v["error"]["kind"], "user_error");
}
