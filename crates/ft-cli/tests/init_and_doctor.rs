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
fn init_writes_embeddings_config_block() {
    // firetrail-6n4: a fresh `firetrail init` seeds the embeddings: section
    // so the daemon and search paths have something explicit to read.
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let init = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(init.success(), "init failed: {}", init.stderr);
    let cfg =
        std::fs::read_to_string(tr.firetrail_dir().join("config.yml")).expect("read config.yml");
    assert!(
        cfg.contains("embeddings:"),
        "missing embeddings block:\n{cfg}"
    );
    assert!(
        cfg.contains("provider: local"),
        "default provider missing:\n{cfg}"
    );
    assert!(
        cfg.contains("model: bge-small-en-v1.5"),
        "default model missing:\n{cfg}"
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

// ── interactive walkthrough ─────────────────────────────────────────────────

use std::io::Write;
use std::process::Stdio;

/// Drive `firetrail init` through the interactive walkthrough by piping
/// scripted stdin and setting `FIRETRAIL_FORCE_TTY=1` so the prompt
/// helpers engage even though `cargo test` doesn't allocate a pty.
fn run_init_interactive(root: &Path, script: &str, args: &[&str]) -> CmdOutput {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let mut cmd = Command::new(bin);
    let mut full = vec!["init"];
    full.extend(args.iter().copied());
    let mut child = cmd
        .args(&full)
        .current_dir(root)
        .env("FIRETRAIL_FORCE_TTY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn firetrail");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait firetrail");
    CmdOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status,
    }
}

#[test]
fn init_interactive_accepts_all_defaults_and_skips_identity() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();

    // Script: storage external? N, register identity? N, hooks? Y, agents? Y,
    // download model? N. (Defaults all accepted via blank lines also work,
    // but explicit answers make the test self-documenting.)
    let script = "n\nn\ny\ny\nn\n";
    let out = run_init_interactive(tr.root(), script, &["--json", "--interactive"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let v: serde_json::Value = serde_json::from_str(&out.stdout).expect("init --json");
    assert_eq!(v["command"], "init");
    assert!(tr.firetrail_dir().join("config.yml").exists());
    assert!(tr.firetrail_dir().join("identity.yml").exists());

    // Identity should NOT have been registered.
    let identities = tr.firetrail_dir().join("identities.yaml");
    if identities.exists() {
        let body = std::fs::read_to_string(&identities).unwrap();
        assert!(
            !body.contains("kind: human"),
            "no identity should be registered: {body}"
        );
    }
}

#[test]
fn init_interactive_registers_identity_from_git_config() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();

    // Seed git config so the walkthrough can pre-populate.
    Command::new("git")
        .args(["config", "user.email", "alice@example.com"])
        .current_dir(tr.root())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Alice"])
        .current_dir(tr.root())
        .status()
        .unwrap();

    // Script: external? N, register? Y, id default (blank), name default
    // (blank), strict? N, hooks? Y, agents? Y, model? N.
    let script = "n\ny\n\n\nn\ny\ny\nn\n";
    let out = run_init_interactive(tr.root(), script, &["--json", "--interactive"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let v: serde_json::Value = serde_json::from_str(&out.stdout).expect("init --json");
    let created = v["data"]["created"].as_array().expect("created array");
    let has_identity = created
        .iter()
        .any(|c| c.as_str().is_some_and(|s| s.starts_with("identity:alice")));
    assert!(
        has_identity,
        "expected identity:alice in created: {created:?}"
    );

    // The registry file should now exist with alice.
    let registry_path = tr.firetrail_dir().join("identities.yaml");
    assert!(registry_path.exists(), "identities.yaml not written");
    let body = std::fs::read_to_string(&registry_path).unwrap();
    assert!(body.contains("alice@example.com"), "email missing: {body}");
}

#[test]
fn init_non_interactive_flag_suppresses_prompts_even_with_force_tty() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();

    // FIRETRAIL_FORCE_TTY=1 would normally engage prompts, but
    // --non-interactive must override. We pipe no stdin script and assert
    // success — if prompts engaged, stdin EOF would still return defaults,
    // but the test verifies no identity registration happened (which only
    // the walkthrough offers).
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["init", "--json", "--non-interactive"])
        .current_dir(tr.root())
        .env("FIRETRAIL_FORCE_TTY", "1")
        .output()
        .expect("spawn firetrail");
    assert!(out.status.success(), "init failed: {out:?}");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let created = v["data"]["created"].as_array().expect("created");
    let has_identity = created
        .iter()
        .any(|c| c.as_str().is_some_and(|s| s.starts_with("identity:")));
    assert!(
        !has_identity,
        "no identity should auto-register: {created:?}"
    );
}

#[test]
fn init_with_behavior_flag_skips_auto_interactive() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();

    // Passing --no-agents must take init off the auto-interactive path
    // even with FIRETRAIL_FORCE_TTY=1. No stdin script provided; success
    // implies init did not block waiting for input.
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(["init", "--json", "--no-agents"])
        .current_dir(tr.root())
        .env("FIRETRAIL_FORCE_TTY", "1")
        .output()
        .expect("spawn firetrail");
    assert!(out.status.success(), "init failed: stderr={:?}", out.stderr);
}

// ── AGENTS.md / CLAUDE.md / SKILL.md ────────────────────────────────────────

#[test]
fn init_fresh_writes_full_agents_claude_and_skill() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    // Ensure no pre-existing agent files leak from TestRepo bootstrap.
    let _ = std::fs::remove_file(tr.root().join("AGENTS.md"));
    let _ = std::fs::remove_file(tr.root().join("CLAUDE.md"));

    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let agents = std::fs::read_to_string(tr.root().join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("<!-- firetrail:begin -->"),
        "no begin marker"
    );
    assert!(agents.contains("<!-- firetrail:end -->"), "no end marker");
    assert!(
        agents.contains("firetrail ready"),
        "workflow content missing"
    );
    assert!(agents.contains("firetrail check pr"), "PR content missing");

    let claude = std::fs::read_to_string(tr.root().join("CLAUDE.md")).unwrap();
    assert!(
        claude.contains("AGENTS.md"),
        "CLAUDE.md should point to AGENTS.md: {claude}"
    );

    let skill =
        std::fs::read_to_string(tr.root().join(".claude/skills/firetrail/SKILL.md")).unwrap();
    assert!(
        skill.contains("name: firetrail"),
        "skill frontmatter missing"
    );
    assert!(
        skill.contains("firetrail check pr"),
        "skill content missing"
    );
}

#[test]
fn init_existing_agents_without_markers_appends_block() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let user_content = "# AGENTS.md\n\n## My team's guidance\n\nSome existing rules here.\n";
    std::fs::write(tr.root().join("AGENTS.md"), user_content).unwrap();

    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let agents = std::fs::read_to_string(tr.root().join("AGENTS.md")).unwrap();
    assert!(
        agents.starts_with("# AGENTS.md\n\n## My team's guidance"),
        "user content lost from top: {agents}"
    );
    assert!(
        agents.contains("Some existing rules here."),
        "user content lost: {agents}"
    );
    assert!(
        agents.contains("<!-- firetrail:begin -->"),
        "block not appended"
    );
    assert!(
        agents.contains("firetrail ready"),
        "workflow content missing"
    );
}

#[test]
fn init_existing_agents_with_markers_refreshes_block_only() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let user_content = "# AGENTS.md\n\n## Local rules\n\nKeep this.\n\n<!-- firetrail:begin -->\nSTALE\n<!-- firetrail:end -->\n\n## Footer\n\nKeep this too.\n";
    std::fs::write(tr.root().join("AGENTS.md"), user_content).unwrap();

    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let agents = std::fs::read_to_string(tr.root().join("AGENTS.md")).unwrap();
    assert!(agents.contains("Keep this."), "pre-block content lost");
    assert!(agents.contains("Keep this too."), "post-block content lost");
    assert!(!agents.contains("STALE"), "stale block content remained");
    assert!(
        agents.contains("firetrail check pr"),
        "fresh content not inserted"
    );
}

#[test]
fn init_idempotent_for_agents_block() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let _ = std::fs::remove_file(tr.root().join("AGENTS.md"));

    let first = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(first.success());
    let agents_v1 = std::fs::read_to_string(tr.root().join("AGENTS.md")).unwrap();

    let second = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(second.success());
    let agents_v2 = std::fs::read_to_string(tr.root().join("AGENTS.md")).unwrap();

    assert_eq!(
        agents_v1, agents_v2,
        "AGENTS.md must be byte-stable across re-init"
    );
    let v: serde_json::Value = serde_json::from_str(&second.stdout).unwrap();
    let preserved = v["data"]["preserved"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect::<Vec<_>>();
    assert!(
        preserved.contains(&"AGENTS.md"),
        "second run should report AGENTS.md preserved: {preserved:?}"
    );
}

#[test]
fn init_preserves_existing_claude_md() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let claude_user = "# CLAUDE.md\n\nMy team's specific Claude guidance.\n";
    std::fs::write(tr.root().join("CLAUDE.md"), claude_user).unwrap();

    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let claude_after = std::fs::read_to_string(tr.root().join("CLAUDE.md")).unwrap();
    assert_eq!(
        claude_user, claude_after,
        "existing CLAUDE.md must not be modified"
    );
}

/// Guard against template drift: pin the command shapes that appear as
/// examples in `AGENTS_FIRETRAIL_BLOCK.md`. If a CLI signature changes
/// without the template being updated, this test fails.
#[test]
fn agents_template_examples_stay_in_sync_with_cli() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let _ = std::fs::remove_file(tr.root().join("AGENTS.md"));
    run_firetrail(tr.root(), &["init"]);

    let agents = std::fs::read_to_string(tr.root().join("AGENTS.md")).unwrap();

    // Each entry: (substring in template, [args]) — args must produce a
    // successful --help invocation, proving the surface still exists.
    let pinned: &[(&str, &[&str])] = &[
        ("firetrail epic    create", &["epic", "create", "--help"]),
        ("firetrail task    create", &["task", "create", "--help"]),
        ("firetrail subtask create", &["subtask", "create", "--help"]),
        ("firetrail bug     create", &["bug", "create", "--help"]),
        ("firetrail criteria add", &["criteria", "add", "--help"]),
        (
            "firetrail dep  add <from-id> <to-id>",
            &["dep", "add", "--help"],
        ),
        ("firetrail link <from> <to>", &["link", "--help"]),
        (
            "firetrail finding  create",
            &["finding", "create", "--help"],
        ),
        (
            "firetrail incident create",
            &["incident", "create", "--help"],
        ),
        (
            "firetrail runbook  create",
            &["runbook", "create", "--help"],
        ),
        (
            "firetrail decision create",
            &["decision", "create", "--help"],
        ),
        ("firetrail gotcha   create", &["gotcha", "create", "--help"]),
        ("firetrail capture  --kind", &["capture", "--help"]),
        ("firetrail memory review", &["memory", "review", "--help"]),
        ("firetrail memory promote", &["memory", "promote", "--help"]),
        (
            "firetrail memory supersede",
            &["memory", "supersede", "--help"],
        ),
        ("firetrail check pr <base-ref>", &["check", "pr", "--help"]),
        ("firetrail verify <id>", &["verify", "--help"]),
        ("firetrail diff <base-ref>", &["diff", "--help"]),
        ("firetrail claim-takeover", &["claim-takeover", "--help"]),
        (
            "firetrail identity register",
            &["identity", "register", "--help"],
        ),
    ];

    let mut missing = Vec::new();
    for (snippet, args) in pinned {
        if !agents.contains(snippet) {
            missing.push(format!("template lost: `{snippet}`"));
            continue;
        }
        let out = run_firetrail(tr.root(), args);
        if !out.success() {
            missing.push(format!(
                "CLI no longer accepts {args:?} (template references `{snippet}`)"
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "AGENTS template drifted from CLI:\n  {}",
        missing.join("\n  ")
    );
}
