//! M5 CLI surface tests: identity registry, scope queries, claim takeover,
//! external storage init, and sync.

mod common;

use std::process::Command;

use common::{fresh_repo, id_from_create, run_firetrail};
use ft_testkit::TestRepo;

// ── identity ──────────────────────────────────────────────────────────────

#[test]
fn identity_register_writes_identities_yaml() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "identity",
            "register",
            "alice",
            "--name",
            "Alice",
            "--emails",
            "alice@example.com,alice@personal.com",
            "--kind",
            "human",
        ],
    );
    assert!(out.success(), "register failed: {}", out.stderr);
    let v = common::parse_json(&out);
    assert_eq!(v["data"]["identity"]["id"], "alice");
    assert!(
        tr.firetrail_dir().join("identities.yaml").exists(),
        "identities.yaml not written"
    );
}

#[test]
fn identity_list_shows_registered() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &[
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
    assert!(out.success(), "register failed: {}", out.stderr);
    let listed = run_firetrail(tr.root(), &["--json", "identity", "list"]);
    assert!(listed.success(), "list failed: {}", listed.stderr);
    let v = common::parse_json(&listed);
    let ids = v["data"]["identities"].as_array().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0]["id"], "alice");
}

#[test]
fn identity_offboard_with_sweep_releases_claims() {
    let tr = fresh_repo();
    // Register a worker identity whose email matches FIRETRAIL_AUTHOR
    // (`tester@firetrail.test`).
    let out = run_firetrail(
        tr.root(),
        &[
            "identity",
            "register",
            "tester",
            "--name",
            "Tester",
            "--emails",
            "tester@firetrail.test",
            "--kind",
            "human",
        ],
    );
    assert!(out.success(), "register failed: {}", out.stderr);

    let id_a = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "a"],
    ));
    let id_b = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "b"],
    ));
    assert!(run_firetrail(tr.root(), &["claim", &id_a]).success());
    assert!(run_firetrail(tr.root(), &["claim", &id_b]).success());

    let off = run_firetrail(
        tr.root(),
        &["--json", "identity", "offboard", "tester", "--sweep-claims"],
    );
    assert!(off.success(), "offboard failed: {}", off.stderr);
    let v = common::parse_json(&off);
    let released = v["data"]["released_claims"].as_array().unwrap();
    assert_eq!(released.len(), 2, "expected both claims released: {v}");
}

// ── claim takeover ────────────────────────────────────────────────────────

#[test]
fn claim_takeover_on_expired_succeeds() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    // Claim with a 1-second expiry (`humantime` accepts "1s").
    let claim = run_firetrail(tr.root(), &["claim", &id, "--expires", "1s"]);
    assert!(claim.success(), "initial claim failed: {}", claim.stderr);
    // Wait past expiry.
    std::thread::sleep(std::time::Duration::from_secs(2));

    // A second actor takes over without --force.
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["--json", "claim-takeover", &id])
        .current_dir(tr.root())
        .env("FIRETRAIL_AUTHOR", "other@firetrail.test")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "takeover should succeed on expired claim, stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn claim_takeover_on_live_without_force_fails() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let claim = run_firetrail(tr.root(), &["claim", &id]);
    assert!(claim.success(), "initial claim failed: {}", claim.stderr);

    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["--json", "claim-takeover", &id])
        .current_dir(tr.root())
        .env("FIRETRAIL_AUTHOR", "other@firetrail.test")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "live takeover without --force should fail"
    );
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn unclaim_takeover_on_expired_releases_claim() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    let claim = run_firetrail(tr.root(), &["claim", &id, "--expires", "1s"]);
    assert!(claim.success(), "initial claim failed: {}", claim.stderr);
    std::thread::sleep(std::time::Duration::from_secs(2));

    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args([
            "--json", "unclaim", &id, "--takeover", "--reason", "expired",
        ])
        .current_dir(tr.root())
        .env("FIRETRAIL_AUTHOR", "other@firetrail.test")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "unclaim --takeover should succeed on expired claim, stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn unclaim_takeover_requires_reason() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["claim", &id]);
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["--json", "unclaim", &id, "--takeover"])
        .current_dir(tr.root())
        .env("FIRETRAIL_AUTHOR", "other@firetrail.test")
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected user error");
}

#[test]
fn unclaim_takeover_on_live_without_admin_fails() {
    let tr = fresh_repo();
    let id = id_from_create(&run_firetrail(
        tr.root(),
        &["--json", "task", "create", "demo"],
    ));
    run_firetrail(tr.root(), &["claim", &id]);
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args([
            "--json", "unclaim", &id, "--takeover", "--reason", "stale",
        ])
        .current_dir(tr.root())
        .env("FIRETRAIL_AUTHOR", "other@firetrail.test")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "unclaim --takeover on live claim without admin should fail"
    );
    assert_eq!(out.status.code(), Some(3));
}

// ── scope ────────────────────────────────────────────────────────────────

fn write_scopes_yaml(tr: &TestRepo) {
    let dir = tr.firetrail_dir();
    let codeowners = dir.join("CODEOWNERS");
    std::fs::write(
        &codeowners,
        "apps/checkout/* alice@example.com bob@example.com\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scopes.yaml"),
        format!(
            "scopes:\n  - id: apps/checkout\n    name: Checkout\n    applies_to:\n      - apps/checkout/**\n    aliases:\n      - checkout\n    codeowners: {}\n",
            codeowners.display()
        ),
    )
    .unwrap();
}

#[test]
fn scope_list_and_show_round_trip() {
    let tr = fresh_repo();
    write_scopes_yaml(&tr);
    let listed = run_firetrail(tr.root(), &["--json", "scope", "list"]);
    assert!(listed.success(), "list failed: {}", listed.stderr);
    let v = common::parse_json(&listed);
    let scopes = v["data"]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0]["id"], "apps/checkout");

    let show = run_firetrail(tr.root(), &["--json", "scope", "show", "checkout"]);
    assert!(show.success(), "show failed: {}", show.stderr);
    let v = common::parse_json(&show);
    assert_eq!(v["data"]["scope"]["id"], "apps/checkout");
}

#[test]
fn scope_owners_resolves_codeowners() {
    let tr = fresh_repo();
    write_scopes_yaml(&tr);
    let out = run_firetrail(
        tr.root(),
        &["--json", "scope", "owners", "apps/checkout/cart.rs"],
    );
    assert!(out.success(), "owners failed: {}", out.stderr);
    let v = common::parse_json(&out);
    let owners = v["data"]["owners"].as_array().unwrap();
    let names: Vec<&str> = owners.iter().filter_map(|x| x.as_str()).collect();
    assert!(names.contains(&"alice@example.com"), "owners: {names:?}");
    assert!(names.contains(&"bob@example.com"), "owners: {names:?}");
}

// ── init --storage-mode external ─────────────────────────────────────────

fn init_bare_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let status = Command::new("git")
        .args(["init", "--bare", "--initial-branch=main"])
        .current_dir(tmp.path())
        .status()
        .unwrap();
    assert!(status.success());
    tmp
}

#[test]
fn init_external_writes_config_and_subsequent_commands_use_external_storage() {
    let bare = init_bare_repo();
    let url = format!("file://{}", bare.path().display());

    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let out = run_firetrail(
        tr.root(),
        &[
            "init",
            "--json",
            "--no-hooks",
            "--no-agents",
            "--storage-mode",
            "external",
            "--data-repo-url",
            &url,
        ],
    );
    assert!(out.success(), "init external failed: {}", out.stderr);
    let cfg = std::fs::read_to_string(tr.firetrail_dir().join("config.yml")).unwrap();
    assert!(cfg.contains("mode: external"), "config: {cfg}");
    assert!(cfg.contains(&url), "config: {cfg}");

    // Doctor should now report external storage; the clone may need to be
    // materialised by a command that opens external storage. Run `sync`
    // which calls ExternalStorage::open.
    let sync = run_firetrail(tr.root(), &["--json", "sync"]);
    assert!(sync.success(), "sync failed: {}", sync.stderr);
    let v = common::parse_json(&sync);
    assert_eq!(v["data"]["pulled"], true);
    assert_eq!(v["data"]["pushed"], true);
    assert!(
        tr.firetrail_dir()
            .join("cache")
            .join("data-repo")
            .join(".git")
            .exists(),
        "clone was not materialised"
    );
}
