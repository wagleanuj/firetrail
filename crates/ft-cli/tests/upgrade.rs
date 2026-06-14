//! Integration coverage for `firetrail upgrade`.
//!
//! The networked install path is not exercised here; we test the hermetic
//! seam: a binary with no install receipt must fail with an actionable user
//! error rather than attempting a network update.
//!
//! We point axoupdater's receipt search at an empty directory via
//! `AXOUPDATER_CONFIG_PATH`, so the test is hermetic regardless of whatever is
//! (or isn't) installed on the host. The workspace forbids `unsafe`, so the env
//! var is set on the child process via `Command::env` rather than mutating the
//! test process environment.

use std::process::Command;

/// In a clean config dir with no install receipt, `upgrade --check` must fail
/// with a clear, non-panicking user error that explains the install-method
/// limitation.
#[test]
fn upgrade_without_receipt_is_a_friendly_user_error() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["upgrade", "--check"])
        .env("AXOUPDATER_CONFIG_PATH", tmp.path())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "no receipt present, expected a failing exit, stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        msg.contains("installer") || msg.contains("install"),
        "error should explain the install-method limitation, got: {msg}"
    );
}
