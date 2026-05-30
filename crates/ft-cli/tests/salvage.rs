//! Integration tests for ADR-0018 — branch salvage workflow.
//!
//! Covers:
//!
//! - `firetrail init` installs the `post-checkout` / `post-merge` hook
//!   regions (managed by the `# >>> firetrail managed >>>` markers) and
//!   re-init is idempotent.
//! - `firetrail memory salvage --auto` salvages memory records, skips
//!   structural records, and produces a stable JSON envelope.
//! - `firetrail memory salvage --dry-run` is a no-op.
//! - With nothing to salvage, the command exits 0 with an empty list.

#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

use std::path::Path;
use std::process::Command;

use common::{parse_json, run_firetrail};
use ft_testkit::{CmdOutput, TestRepo};

// ── helpers ─────────────────────────────────────────────────────────────────

/// Fresh `TestRepo` + `firetrail init` *with* hooks installed (unlike the
/// shared `fresh_repo` helper which passes `--no-hooks`).
fn fresh_repo_with_hooks() -> TestRepo {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let init = run_firetrail(tr.root(), &["init", "--json", "--no-agents"]);
    assert!(init.success(), "init failed: {}", init.stderr);
    tr
}

/// Run `git` in `root` and return captured stdout, asserting success.
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Commit everything currently in the working tree under `repo`.
fn commit_all(root: &Path, msg: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", msg]);
}

/// Create one record of `kind` on the current branch via `firetrail`.
fn create_record(root: &Path, kind: &str, title: &str) -> String {
    let args: Vec<&str> = match kind {
        "finding" => vec!["--json", "finding", "create", title],
        "task" => vec!["--json", "task", "create", title],
        other => panic!("unsupported kind for test helper: {other}"),
    };
    let out = run_firetrail(root, &args);
    assert!(out.success(), "create {kind} failed: {}", out.stderr);
    let v = parse_json(&out);
    v["data"]["record"]["envelope"]["id"]
        .as_str()
        .expect("envelope.id")
        .to_string()
}

fn read_hook(root: &Path, name: &str) -> String {
    let path = root.join(".git/hooks").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ── hooks: installation & idempotency ───────────────────────────────────────

#[test]
fn init_installs_post_checkout_and_post_merge_hooks() {
    let tr = fresh_repo_with_hooks();

    let post_checkout = read_hook(tr.root(), "post-checkout");
    assert!(
        post_checkout.contains("# >>> firetrail managed >>>"),
        "post-checkout missing managed-begin marker:\n{post_checkout}"
    );
    assert!(
        post_checkout.contains("# <<< firetrail managed <<<"),
        "post-checkout missing managed-end marker"
    );
    assert!(
        post_checkout.contains("firetrail _hook on-checkout"),
        "post-checkout body does not invoke `_hook on-checkout`:\n{post_checkout}"
    );

    let post_merge = read_hook(tr.root(), "post-merge");
    assert!(
        post_merge.contains("# >>> firetrail managed >>>"),
        "post-merge missing managed marker"
    );
    assert!(
        post_merge.contains("firetrail _hook on-merge"),
        "post-merge body does not invoke `_hook on-merge`"
    );
}

#[test]
fn reinit_is_idempotent_for_hooks() {
    let tr = fresh_repo_with_hooks();
    let first_post_checkout = read_hook(tr.root(), "post-checkout");

    // Re-run init; the managed block should remain a single contiguous region.
    let again = run_firetrail(tr.root(), &["init", "--json", "--no-agents"]);
    assert!(again.success(), "re-init failed: {}", again.stderr);

    let second = read_hook(tr.root(), "post-checkout");
    assert_eq!(
        second.matches("# >>> firetrail managed >>>").count(),
        1,
        "managed-begin marker should appear exactly once:\n{second}"
    );
    assert_eq!(
        second.matches("# <<< firetrail managed <<<").count(),
        1,
        "managed-end marker should appear exactly once"
    );
    // Content of the managed block should be unchanged across re-inits.
    assert_eq!(first_post_checkout, second);
}

#[test]
fn _hook_is_hidden_from_help() {
    let tr = TestRepo::new().unwrap();
    let out = run_firetrail(tr.root(), &["--help"]);
    assert!(out.success(), "--help failed: {}", out.stderr);
    assert!(
        !out.stdout.contains("_hook"),
        "`_hook` should be hidden from --help output:\n{}",
        out.stdout
    );
}

// ── salvage: end-to-end auto ────────────────────────────────────────────────

#[test]
fn salvage_auto_salvages_memory_records_and_skips_task() {
    let tr = fresh_repo_with_hooks();
    // Commit init artifacts so `main` is in a known state before branching.
    commit_all(tr.root(), "init firetrail");

    // Branch off and create 3 findings + 1 task on the feature branch.
    git(tr.root(), &["checkout", "--quiet", "-b", "feat/scope"]);

    let f1 = create_record(tr.root(), "finding", "redis pool exhaustion");
    let f2 = create_record(tr.root(), "finding", "checkout retry storm");
    let f3 = create_record(tr.root(), "finding", "db connection cap");
    let t1 = create_record(tr.root(), "task", "add redis saturation alert");
    commit_all(tr.root(), "feature work in progress");

    // Salvage.
    let out = run_firetrail(
        tr.root(),
        &["--json", "memory", "salvage", "--auto", "--base", "main"],
    );
    assert!(
        out.success(),
        "memory salvage failed: stdout={}\nstderr={}",
        out.stdout,
        out.stderr
    );
    let v = parse_json(&out);
    assert_eq!(v["command"], "memory salvage");
    let data = &v["data"];
    assert_eq!(data["base"], "main");
    assert_eq!(data["source_branch"], "feat/scope");
    assert_eq!(data["dry_run"], false);

    let entries = data["entries"]
        .as_array()
        .expect("entries is an array")
        .clone();
    assert_eq!(
        entries.len(),
        4,
        "expected 4 candidate entries: {entries:?}"
    );

    let salvaged: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["action"] == "salvaged")
        .collect();
    let skipped: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["action"] == "skipped")
        .collect();
    assert_eq!(salvaged.len(), 3, "expected 3 salvaged: {entries:?}");
    assert_eq!(skipped.len(), 1, "expected 1 skipped: {entries:?}");

    for kind_entry in &salvaged {
        assert_eq!(kind_entry["kind"], "finding", "non-finding salvaged");
    }
    for kind_entry in &skipped {
        assert_eq!(kind_entry["kind"], "task", "non-task skipped");
    }

    let salvaged_ids: Vec<&str> = salvaged.iter().filter_map(|e| e["id"].as_str()).collect();
    for fid in [&f1, &f2, &f3] {
        assert!(
            salvaged_ids.iter().any(|s| *s == fid),
            "expected {fid} in salvaged ids: {salvaged_ids:?}"
        );
    }
    let skipped_ids: Vec<&str> = skipped.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(skipped_ids.contains(&t1.as_str()));

    // The salvage branch was created and contains the 3 finding blobs.
    let branch = data["salvage_branch"]
        .as_str()
        .expect("salvage_branch present");
    assert!(
        branch.starts_with("salvage/main-from-feat/scope-"),
        "unexpected salvage branch name: {branch}"
    );

    let branches = git(tr.root(), &["branch", "--list", branch]);
    assert!(
        branches.contains(branch),
        "salvage branch not created: {branches}"
    );

    // The operator's HEAD is back on the source branch.
    let current = git(tr.root(), &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(current.trim(), "feat/scope");

    // Inspect the salvage branch's tree: should contain 3 finding files,
    // no task file.
    let ls = git(
        tr.root(),
        &[
            "ls-tree",
            "-r",
            "--name-only",
            branch,
            ".firetrail/records/",
        ],
    );
    let finding_count = ls
        .lines()
        .filter(|l| l.starts_with(".firetrail/records/finding/"))
        .count();
    let task_count = ls
        .lines()
        .filter(|l| l.starts_with(".firetrail/records/task/"))
        .count();
    assert_eq!(finding_count, 3, "expected 3 findings on salvage: {ls}");
    assert_eq!(task_count, 0, "expected no tasks on salvage: {ls}");
}

// ── salvage: dry-run does not mutate ────────────────────────────────────────

#[test]
fn salvage_dry_run_does_not_create_a_branch() {
    let tr = fresh_repo_with_hooks();
    commit_all(tr.root(), "init firetrail");
    git(tr.root(), &["checkout", "--quiet", "-b", "feat/dry"]);
    create_record(tr.root(), "finding", "a");
    create_record(tr.root(), "finding", "b");
    commit_all(tr.root(), "feature work");

    let before_branches = git(tr.root(), &["branch", "--list"]);
    let out = run_firetrail(
        tr.root(),
        &["--json", "memory", "salvage", "--dry-run", "--base", "main"],
    );
    assert!(out.success(), "dry-run failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["dry_run"], true);
    assert!(
        v["data"]["salvage_branch"].is_null(),
        "dry-run must not set salvage_branch: {}",
        v["data"]
    );

    let entries = v["data"]["entries"].as_array().expect("entries");
    let salvaged = entries.iter().filter(|e| e["action"] == "salvaged").count();
    assert_eq!(
        salvaged, 2,
        "dry-run should still report intent: {entries:?}"
    );

    // No salvage branch was created.
    let after_branches = git(tr.root(), &["branch", "--list"]);
    assert_eq!(
        before_branches, after_branches,
        "dry-run mutated the branch list:\nbefore={before_branches}\nafter={after_branches}"
    );

    // Working tree is still clean.
    let status = git(tr.root(), &["status", "--porcelain"]);
    assert!(
        status.trim().is_empty(),
        "dry-run dirtied the working tree: {status}"
    );
}

// ── salvage: empty case is success ──────────────────────────────────────────

#[test]
fn salvage_with_nothing_to_do_is_ok_with_empty_list() {
    let tr = fresh_repo_with_hooks();
    commit_all(tr.root(), "init firetrail");
    git(tr.root(), &["checkout", "--quiet", "-b", "feat/empty"]);
    // No records created on this branch.

    let out = run_firetrail(
        tr.root(),
        &["--json", "memory", "salvage", "--auto", "--base", "main"],
    );
    assert!(
        out.success(),
        "salvage on empty branch should succeed: {}",
        out.stderr
    );
    let v = parse_json(&out);
    let entries = v["data"]["entries"].as_array().expect("entries");
    assert!(
        entries.is_empty(),
        "expected empty salvage entries: {entries:?}"
    );
    assert!(v["data"]["salvage_branch"].is_null());
}

// ── salvage: --non-interactive is canonical, --auto is a deprecated alias ────

/// Both `--non-interactive` (canonical) and `--auto` (deprecated alias) drive
/// the same non-interactive salvage, and passing them together no longer
/// errors with a `conflicts_with` clash.
#[test]
fn salvage_non_interactive_and_auto_alias_both_work_without_conflict() {
    for flags in [
        vec!["--non-interactive"],
        vec!["--auto"],
        vec!["--auto", "--non-interactive"],
    ] {
        let tr = fresh_repo_with_hooks();
        commit_all(tr.root(), "init firetrail");
        git(tr.root(), &["checkout", "--quiet", "-b", "feat/aliases"]);
        create_record(tr.root(), "finding", "alias-driven finding");
        commit_all(tr.root(), "feature work");

        let mut argv = vec!["--json", "memory", "salvage"];
        argv.extend(flags.iter().copied());
        argv.extend(["--base", "main"]);
        let out = run_firetrail(tr.root(), &argv);
        assert!(
            out.success(),
            "salvage with {flags:?} should succeed: stdout={}\nstderr={}",
            out.stdout,
            out.stderr
        );
        let v = parse_json(&out);
        let entries = v["data"]["entries"].as_array().expect("entries");
        let salvaged = entries.iter().filter(|e| e["action"] == "salvaged").count();
        assert_eq!(
            salvaged, 1,
            "{flags:?} should non-interactively salvage the finding: {entries:?}"
        );
    }
}

// Force `CmdOutput` usage so its import remains explicitly required even
// once the helpers above are inlined in the future. Keeps the diff minimal
// when more tests land.
#[allow(dead_code)]
fn _force_cmd_output_used(_o: &CmdOutput) {}
