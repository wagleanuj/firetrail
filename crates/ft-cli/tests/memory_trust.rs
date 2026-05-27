//! Trust transition tests — promote, deprecate, supersede, redact, plus the
//! ADR-0013 "needs two reviewers for Verified" gate.

mod common;

use std::process::Command;

use common::{fresh_repo, parse_json};

/// Helper: run firetrail with a specific actor email.
fn run_as(root: &std::path::Path, who: &str, args: &[&str]) -> ft_testkit::CmdOutput {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let out = Command::new(bin)
        .args(args)
        .current_dir(root)
        .env("FIRETRAIL_AUTHOR", who)
        .output()
        .expect("spawn firetrail");
    ft_testkit::CmdOutput {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        status: out.status,
    }
}

fn create_finding(root: &std::path::Path, author: &str) -> String {
    let out = run_as(
        root,
        author,
        &["--json", "finding", "create", "leaked secret in logs"],
    );
    assert!(out.success(), "finding create: {}", out.stderr);
    parse_json(&out)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn review_promotes_draft_to_reviewed_by_distinct_reviewer() {
    let tr = fresh_repo();
    let id = create_finding(tr.root(), "alice@firetrail.test");
    // Reviewer must differ from author.
    let out = run_as(
        tr.root(),
        "bob@firetrail.test",
        &["--json", "memory", "review", &id, "--reason", "looks good"],
    );
    assert!(out.success(), "review failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["body"]["trust"], "reviewed");
}

#[test]
fn review_rejects_self_review() {
    let tr = fresh_repo();
    let id = create_finding(tr.root(), "alice@firetrail.test");
    let out = run_as(
        tr.root(),
        "alice@firetrail.test",
        &["--json", "memory", "review", &id],
    );
    assert!(!out.success(), "self-review should fail");
    assert_eq!(out.status.code(), Some(1));
}

/// Headline: a finding cannot reach Verified without a second reviewer.
#[test]
fn promote_requires_two_distinct_reviewers() {
    let tr = fresh_repo();
    let id = create_finding(tr.root(), "alice@firetrail.test");
    // Bob reviews (Draft -> Reviewed).
    let r1 = run_as(
        tr.root(),
        "bob@firetrail.test",
        &["--json", "memory", "review", &id],
    );
    assert!(r1.success(), "first review failed: {}", r1.stderr);
    // Bob tries to promote himself — should fail (DuplicateReviewer).
    let dup = run_as(
        tr.root(),
        "bob@firetrail.test",
        &["--json", "memory", "promote", &id],
    );
    assert!(!dup.success(), "duplicate reviewer should fail");
    assert_eq!(dup.status.code(), Some(1));

    // Carol promotes — succeeds.
    let ok = run_as(
        tr.root(),
        "carol@firetrail.test",
        &["--json", "memory", "promote", &id],
    );
    assert!(ok.success(), "carol promote failed: {}", ok.stderr);
    let v = parse_json(&ok);
    assert_eq!(v["data"]["record"]["body"]["trust"], "verified");
}

#[test]
fn deprecate_requires_reason() {
    let tr = fresh_repo();
    let id = create_finding(tr.root(), "alice@firetrail.test");
    let out = run_as(
        tr.root(),
        "bob@firetrail.test",
        &[
            "--json",
            "memory",
            "deprecate",
            &id,
            "--reason",
            "no longer applicable",
        ],
    );
    assert!(out.success(), "deprecate failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["body"]["trust"], "deprecated");
}

#[test]
fn supersede_links_successor_and_marks_terminal() {
    let tr = fresh_repo();
    let old = create_finding(tr.root(), "alice@firetrail.test");
    let new = create_finding(tr.root(), "alice@firetrail.test");
    let out = run_as(
        tr.root(),
        "bob@firetrail.test",
        &[
            "--json",
            "memory",
            "supersede",
            &old,
            "--with",
            &new,
            "--reason",
            "replaced by ${NEW}",
        ],
    );
    assert!(out.success(), "supersede failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["body"]["trust"], "superseded");
}

#[test]
fn redact_wipes_body_content() {
    let tr = fresh_repo();
    let id = create_finding(tr.root(), "alice@firetrail.test");
    let out = run_as(
        tr.root(),
        "bob@firetrail.test",
        &["--json", "memory", "redact", &id, "--reason", "PII leaked"],
    );
    assert!(out.success(), "redact failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["record"]["body"]["trust"], "redacted");
    // Body content has been wiped by ft-trust.
    assert_eq!(v["data"]["record"]["body"]["summary"], "");
}

#[test]
fn high_stakes_promote_without_evidence_fails() {
    let tr = fresh_repo();
    let out = run_as(
        tr.root(),
        "alice@firetrail.test",
        &[
            "--json",
            "finding",
            "create",
            "rce in upload handler",
            "--risk-class",
            "security",
        ],
    );
    let id = parse_json(&out)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r1 = run_as(
        tr.root(),
        "bob@firetrail.test",
        &["--json", "memory", "review", &id],
    );
    assert!(r1.success(), "review: {}", r1.stderr);
    // No --evidence-url for a high-stakes promotion → reject.
    let p = run_as(
        tr.root(),
        "carol@firetrail.test",
        &["--json", "memory", "promote", &id],
    );
    assert!(!p.success(), "promote without evidence should fail");
    assert_eq!(p.status.code(), Some(1));
    // With evidence URL, succeeds.
    let p2 = run_as(
        tr.root(),
        "carol@firetrail.test",
        &[
            "--json",
            "memory",
            "promote",
            &id,
            "--evidence-url",
            "https://example.com/postmortem",
            "--evidence-type",
            "pull_request",
        ],
    );
    assert!(p2.success(), "promote with evidence failed: {}", p2.stderr);
}

#[test]
fn merge_supersedes_all_others() {
    let tr = fresh_repo();
    let canonical = create_finding(tr.root(), "alice@firetrail.test");
    let dup1 = create_finding(tr.root(), "alice@firetrail.test");
    let dup2 = create_finding(tr.root(), "alice@firetrail.test");
    let out = run_as(
        tr.root(),
        "bob@firetrail.test",
        &["--json", "memory", "merge", &canonical, &dup1, &dup2],
    );
    assert!(out.success(), "merge failed: {}", out.stderr);
}
