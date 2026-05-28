//! M4 CLI surface integration tests:
//!
//! - `check pr` on clean / mixed / strict diffs
//! - `diff` listing of changed records
//! - `lint memory` detecting a tampered record
//! - `review <id>` JSON shape
//! - `merge-driver-install` idempotency + side-effects
//! - the `firetrail-merge-driver` binary on a real three-way merge

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::{fresh_repo, parse_json, run_firetrail};
use ft_storage::{EmbeddedStorage, Storage};
use ft_testkit::TestRepo;

/// Run `git ...` inside `root`, return stdout. Panic on failure.
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn head_sha(root: &std::path::Path) -> String {
    git(root, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Stage everything and commit; returns the new HEAD sha.
fn commit_all(root: &std::path::Path, msg: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", msg]);
    head_sha(root)
}

// ---------------------------------------------------------------------------
// check pr
// ---------------------------------------------------------------------------

/// Helper: commit `firetrail init` artifacts (`.firetrail/`, `.gitignore`)
/// so subsequent record commits diff cleanly against `base`.
fn commit_init_artifacts(root: &std::path::Path) -> String {
    commit_all(root, "seed firetrail workspace")
}

#[test]
fn check_pr_clean_memory_record_exits_zero() {
    let tr = fresh_repo();
    let base = commit_init_artifacts(tr.root());

    // Create a memory record on a branch — the create command writes it to
    // disk but does not auto-commit.
    let create = run_firetrail(
        tr.root(),
        &[
            "--json",
            "memory",
            "create",
            "weekly note",
            "--body",
            "nothing dangerous here",
        ],
    );
    assert!(create.success(), "memory create: {}", create.stderr);

    let head = commit_all(tr.root(), "add memory");

    let out = run_firetrail(tr.root(), &["--json", "check", "pr", &base, &head]);
    assert!(out.success(), "check pr: {}\n{}", out.stdout, out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["report"]["summary"]["errors"], 0);
    assert!(
        v["data"]["report"]["summary"]["changed_records"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn check_pr_mixed_commit_exits_nonzero_with_mixed_finding() {
    let tr = fresh_repo();
    let base = commit_init_artifacts(tr.root());

    // Memory record + code file in the same commit -> MixedCommit.
    run_firetrail(
        tr.root(),
        &[
            "--json",
            "memory",
            "create",
            "mixed",
            "--body",
            "nothing dangerous here",
        ],
    );
    std::fs::write(tr.root().join("hello.txt"), b"hi\n").unwrap();
    let head = commit_all(tr.root(), "mixed");

    let out = run_firetrail(tr.root(), &["--json", "check", "pr", &base, &head]);
    assert!(!out.success(), "expected non-zero exit");
    // Error envelope: details contains the report.
    let v: serde_json::Value = serde_json::from_str(out.stderr.trim()).unwrap_or_else(|e| {
        panic!("not JSON stderr: {e}: {}", out.stderr);
    });
    let rules: Vec<String> = v["error"]["details"]["report"]["findings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|f| f["rule"].as_str().map(str::to_string))
        .collect();
    assert!(
        rules.iter().any(|r| r == "mixed_commit"),
        "expected mixed_commit finding, got rules: {rules:?}"
    );
}

#[test]
fn check_pr_strict_promotes_warnings_to_errors() {
    let tr = fresh_repo();
    let base = commit_init_artifacts(tr.root());

    // Create a task with >10 acceptance criteria. AC cap is a warning by
    // default; --strict should promote it to a blocking error.
    let create = run_firetrail(tr.root(), &["--json", "task", "create", "many-acs"]);
    assert!(create.success(), "task create: {}", create.stderr);
    let task_id = parse_json(&create)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for i in 0..12 {
        let out = run_firetrail(
            tr.root(),
            &[
                "--json",
                "criteria",
                "add",
                &task_id,
                &format!("criterion {i}"),
            ],
        );
        assert!(out.success(), "criteria add: {}", out.stderr);
    }

    let head = commit_all(tr.root(), "task with too many acs");

    // Non-strict run: clean exit, warning surfaced.
    let lax = run_firetrail(tr.root(), &["--json", "check", "pr", &base, &head]);
    assert!(lax.success(), "non-strict should pass: {}", lax.stderr);

    // Strict run: non-zero exit.
    let strict = run_firetrail(
        tr.root(),
        &["--json", "check", "pr", &base, &head, "--strict"],
    );
    assert!(
        !strict.success(),
        "strict should fail when warnings present (stdout: {})",
        strict.stdout
    );
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

#[test]
fn diff_lists_changed_records() {
    let tr = fresh_repo();
    let base = commit_init_artifacts(tr.root());

    run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "n1", "--body", "first"],
    );
    let head = commit_all(tr.root(), "add memory");

    let out = run_firetrail(tr.root(), &["--json", "diff", &base, &head]);
    assert!(out.success(), "diff failed: {}", out.stderr);
    let v = parse_json(&out);
    let rows = v["data"]["rows"].as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "expected at least one row");
    let kinds: Vec<String> = rows
        .iter()
        .filter_map(|r| r["kind"].as_str().map(str::to_string))
        .collect();
    assert!(
        kinds.iter().any(|k| k == "memory"),
        "expected at least one memory-kind row, got {kinds:?}"
    );

    // --memory filter narrows to memory records.
    let mem = run_firetrail(tr.root(), &["--json", "diff", &base, &head, "--memory"]);
    assert!(mem.success());
    let mv = parse_json(&mem);
    let mrows = mv["data"]["rows"].as_array().cloned().unwrap_or_default();
    assert!(mrows.iter().all(|r| r["class"] == "memory"));

    // firetrail-du8: directory entries surfaced by git's tree walk
    // (e.g. `.firetrail`, `.firetrail/records`) must not appear as rows.
    let paths: Vec<String> = rows
        .iter()
        .filter_map(|r| r["path"].as_str().map(str::to_string))
        .collect();
    for forbidden in [".firetrail", ".firetrail/records"] {
        assert!(
            !paths.iter().any(|p| p == forbidden),
            "directory entry leaked into diff rows: {forbidden} (paths={paths:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// lint memory
// ---------------------------------------------------------------------------

#[test]
fn lint_memory_detects_tampered_chain() {
    let tr = fresh_repo();

    let create = run_firetrail(
        tr.root(),
        &[
            "--json",
            "memory",
            "create",
            "tamper-target",
            "--body",
            "clean body",
        ],
    );
    assert!(create.success(), "memory create: {}", create.stderr);
    let id = parse_json(&create)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Tamper directly on disk: rewrite the title without updating state_hash.
    let storage = EmbeddedStorage::open(tr.root()).unwrap();
    let path = storage.path_for(&ft_core::RecordId::from_string(id.clone()).unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["envelope"]["title"] = serde_json::json!("tampered");
    std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

    let out = run_firetrail(tr.root(), &["--json", "lint", "memory"]);
    assert!(!out.success(), "lint should flag tampered record");
    let v: serde_json::Value = serde_json::from_str(out.stderr.trim()).unwrap_or_else(|e| {
        panic!("not JSON: {e}: {}", out.stderr);
    });
    let rules: Vec<String> = v["error"]["details"]["findings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|f| f["rule"].as_str().map(str::to_string))
        .collect();
    assert!(
        rules.iter().any(|r| r == "chain_broken"),
        "expected chain_broken, got {rules:?}"
    );
}

#[test]
fn lint_memory_fix_emits_remediation_hints() {
    let tr = fresh_repo();

    let create = run_firetrail(
        tr.root(),
        &[
            "--json",
            "memory",
            "create",
            "tamper-target",
            "--body",
            "clean body",
        ],
    );
    assert!(create.success(), "memory create: {}", create.stderr);
    let id = parse_json(&create)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let storage = EmbeddedStorage::open(tr.root()).unwrap();
    let path = storage.path_for(&ft_core::RecordId::from_string(id).unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["envelope"]["title"] = serde_json::json!("tampered");
    std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

    let out = run_firetrail(tr.root(), &["--json", "lint", "memory", "--fix"]);
    assert!(!out.success(), "tampered lint must still fail");
    let v: serde_json::Value = serde_json::from_str(out.stderr.trim()).unwrap();
    let hints: Vec<&serde_json::Value> = v["error"]["details"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| !f["suggested_fix"].is_null())
        .collect();
    assert!(
        !hints.is_empty(),
        "--fix must emit at least one suggested_fix: {v}"
    );
}

// ---------------------------------------------------------------------------
// review
// ---------------------------------------------------------------------------

#[test]
fn review_renders_full_record_envelope() {
    let tr = fresh_repo();
    let create = run_firetrail(
        tr.root(),
        &[
            "--json",
            "finding",
            "create",
            "checkout 502s",
            "--details",
            "burst above 5k rps",
            "--risk-class",
            "availability",
        ],
    );
    assert!(create.success(), "finding create: {}", create.stderr);
    let id = parse_json(&create)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = run_firetrail(tr.root(), &["--json", "review", &id]);
    assert!(out.success(), "review: {}", out.stderr);
    let v = parse_json(&out);
    let data = &v["data"];
    assert_eq!(data["id"].as_str(), Some(id.as_str()));
    assert_eq!(data["kind"], "finding");
    assert_eq!(data["risk_class"], "availability");
    assert_eq!(data["trust_state"], "draft");
    assert_eq!(data["chain_valid"], true);
    assert!(data["state_hash"].as_str().unwrap().len() == 64);
    assert!(
        data["suggested_next_action"]
            .as_str()
            .unwrap()
            .contains("review")
    );
    // History timeline is present (genesis Create entry).
    assert!(data["history"].as_array().map_or(0, Vec::len) >= 1);
}

// ---------------------------------------------------------------------------
// merge-driver-install
// ---------------------------------------------------------------------------

#[test]
fn merge_driver_install_writes_gitattributes_and_config_idempotently() {
    let tr = fresh_repo();

    // First run.
    let first = run_firetrail(tr.root(), &["--json", "merge-driver-install"]);
    assert!(first.success(), "first install: {}", first.stderr);
    let v1 = parse_json(&first);
    assert_eq!(v1["data"]["added_gitattributes"], true);
    assert_eq!(v1["data"]["added_git_config"], true);

    let gitattributes = std::fs::read_to_string(tr.root().join(".gitattributes")).unwrap();
    assert!(
        gitattributes.contains(".firetrail/records/**/*.json merge=firetrail"),
        "gitattributes:\n{gitattributes}"
    );
    let git_config = std::fs::read_to_string(tr.root().join(".git").join("config")).unwrap();
    assert!(
        git_config.contains("[merge \"firetrail\"]"),
        "git config:\n{git_config}"
    );
    assert!(git_config.contains("driver = firetrail-merge-driver %O %A %B"));

    // Second run: idempotent.
    let second = run_firetrail(tr.root(), &["--json", "merge-driver-install"]);
    assert!(second.success(), "second install: {}", second.stderr);
    let v2 = parse_json(&second);
    assert_eq!(v2["data"]["added_gitattributes"], false);
    assert_eq!(v2["data"]["added_git_config"], false);
}

// ---------------------------------------------------------------------------
// merge driver binary on a real three-way merge
// ---------------------------------------------------------------------------

#[test]
fn merge_driver_binary_handles_three_way_merge() {
    // Create a base record and write its bytes; then synthesize two divergent
    // edits in tempfiles and run the merge driver binary against them.
    let tr = TestRepo::new().unwrap();
    let storage = EmbeddedStorage::open(tr.root()).unwrap();

    let mut record = ft_testkit::make_task().title("orig").build();
    let path = storage.write(&record).unwrap();

    // Three side temp files for git's %O %A %B convention.
    let base_path = tr.root().join("base.json");
    let ours_path = tr.root().join("ours.json");
    let theirs_path = tr.root().join("theirs.json");

    let base_bytes = std::fs::read(&path).unwrap();
    std::fs::write(&base_path, &base_bytes).unwrap();

    // ours: change title.
    let mut ours = record.clone();
    ours.envelope.title = "ours-title".into();
    ours.envelope.state_hash = String::new();
    ours.envelope.state_hash = ft_core::state_hash(&ours).unwrap();
    std::fs::write(&ours_path, serde_json::to_vec_pretty(&ours).unwrap()).unwrap();

    // theirs: add an acceptance criterion (and re-hash).
    if let ft_core::RecordBody::Task(t) = &mut record.body {
        t.acceptance_criteria.push(ft_core::AcceptanceCriterion {
            id: "ac-01".into(),
            text: "new ac from theirs".into(),
            status: ft_core::AcStatus::Unchecked,
            evidence_url: None,
            checked_by: None,
            checked_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            proposed: false,
        });
    }
    record.envelope.state_hash = String::new();
    record.envelope.state_hash = ft_core::state_hash(&record).unwrap();
    std::fs::write(&theirs_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

    // Run the merge driver binary.
    let bin = env!("CARGO_BIN_EXE_firetrail-merge-driver");
    let status = Command::new(bin)
        .arg(&base_path)
        .arg(&ours_path)
        .arg(&theirs_path)
        .status()
        .expect("spawn merge driver");
    assert!(status.success(), "expected clean merge, exit={status:?}");

    // The merged bytes were written to ours_path. Decode and assert both
    // edits survived.
    let merged_bytes = std::fs::read(&ours_path).unwrap();
    let merged: ft_core::Record = serde_json::from_slice(&merged_bytes).unwrap();
    assert_eq!(merged.envelope.title, "ours-title");
    if let ft_core::RecordBody::Task(t) = &merged.body {
        assert_eq!(t.acceptance_criteria.len(), 1);
        assert_eq!(t.acceptance_criteria[0].text, "new ac from theirs");
    } else {
        panic!("expected task body");
    }
}

// ---------------------------------------------------------------------------
// server-hooks install
// ---------------------------------------------------------------------------

#[test]
fn server_hooks_install_writes_pre_receive_to_dest() {
    let tr = fresh_repo();
    let dest: PathBuf = tr.root().join("custom-hooks");

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "server-hooks",
            "install",
            "--dest",
            dest.to_str().unwrap(),
        ],
    );
    assert!(out.success(), "server-hooks install: {}", out.stderr);
    let hook_path = dest.join("pre-receive");
    assert!(hook_path.exists(), "{} should exist", hook_path.display());
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("firetrail check pr"));
}
