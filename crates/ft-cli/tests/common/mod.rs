//! Shared helpers for ft-cli integration tests.

use std::path::Path;
use std::process::Command;

use ft_testkit::{CmdOutput, TestRepo};

/// Invoke the built `firetrail` binary in `root`.
pub fn run_firetrail(root: &Path, args: &[&str]) -> CmdOutput {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(root);
    // Ensure FIRETRAIL_AUTHOR is always set so identity resolution is
    // deterministic across hosts.
    if std::env::var("FIRETRAIL_AUTHOR").is_err() {
        cmd.env("FIRETRAIL_AUTHOR", "tester@firetrail.test");
    }
    let output = cmd.output().expect("spawn firetrail");
    CmdOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status,
    }
}

/// `firetrail init` a fresh `TestRepo` and return the repo.
pub fn fresh_repo() -> TestRepo {
    let tr = TestRepo::new().unwrap();
    // TestRepo already creates .firetrail/records/<kind>/ skeletons, but the
    // index.db / config.yml come from `firetrail init`.
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let init = run_firetrail(tr.root(), &["init", "--json", "--no-hooks", "--no-agents"]);
    assert!(init.success(), "init failed: {}", init.stderr);
    tr
}

/// Parse the JSON envelope from stdout.
pub fn parse_json(out: &CmdOutput) -> serde_json::Value {
    let trimmed = out.stdout.trim();
    serde_json::from_str(trimmed).unwrap_or_else(|e| {
        panic!(
            "not JSON: {e}\nstdout={}\nstderr={}",
            out.stdout, out.stderr
        )
    })
}

/// Pull the record id out of a successful create-command JSON envelope.
pub fn id_from_create(out: &CmdOutput) -> String {
    assert!(out.success(), "create failed: {}", out.stderr);
    let v = parse_json(out);
    v["data"]["record"]["envelope"]["id"]
        .as_str()
        .expect("envelope.id present")
        .to_string()
}
