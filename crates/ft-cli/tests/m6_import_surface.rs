//! Integration tests for the M6 CLI surface: `import …`, `promote-import`,
//! and the quarantine filter on `search` / `prime`.

mod common;

use std::fs;
use std::path::Path;

use common::{fresh_repo, parse_json, run_firetrail};

const INCIDENT_A: &str = "# Quokka outage 2026-01-01\n\n## Symptoms\n\nQuokka requests timed out.\n\n## Root Cause\n\nBad config.\n\n## Resolution\n\nReverted.\n";
const INCIDENT_B: &str = "# Quokka 2 outage\n\n## Symptoms\n\nQuokka v2 noise.\n";
const INCIDENT_C: &str = "# Quokka 3 outage\n\n## Symptoms\n\nMore quokka noise.\n";

const ADR_FULL: &str = "# ADR-0007: Use quokka clusters\n\n## Status\n\nAccepted\n\n## Context\n\nWe need clusters.\n\n## Decision\n\nAdopt quokka clustering.\n\n## Consequences\n\nMore ops work.\n";

const RUNBOOK_FULL: &str = "# Restart the quokka cluster\n\n## Summary\n\nUse when the cluster is wedged.\n\n## Applies To\n\n- quokka-prod\n\n## Steps\n\n1. Notify oncall\n2. Drain traffic\n3. Restart nodes\n";

fn stage_dir(root: &Path, sub: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = root.join(sub);
    fs::create_dir_all(&dir).unwrap();
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
    dir
}

#[test]
fn import_incidents_dry_run_writes_nothing() {
    let tr = fresh_repo();
    let dir = stage_dir(
        tr.root(),
        "imports-dry",
        &[
            ("a.md", INCIDENT_A),
            ("b.md", INCIDENT_B),
            ("c.md", INCIDENT_C),
        ],
    );

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "import",
            "incidents",
            dir.to_str().unwrap(),
            "--dry-run",
        ],
    );
    assert!(out.success(), "dry-run import failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["parsed"], 3);
    assert_eq!(v["data"]["written"], 0);
    assert_eq!(v["data"]["dry_run"], true);

    // No incident records on disk.
    let inc_dir = tr.root().join(".firetrail/records/incident");
    if inc_dir.exists() {
        let count = fs::read_dir(&inc_dir).unwrap().count();
        assert_eq!(count, 0, "dry-run should not write");
    }
}

#[test]
fn import_incidents_apply_writes_quarantined() {
    let tr = fresh_repo();
    let dir = stage_dir(
        tr.root(),
        "imports-apply",
        &[("a.md", INCIDENT_A), ("b.md", INCIDENT_B)],
    );

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "import",
            "incidents",
            dir.to_str().unwrap(),
            "--apply",
        ],
    );
    assert!(out.success(), "apply import failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["parsed"], 2);
    assert_eq!(v["data"]["written"], 2);
    assert_eq!(v["data"]["apply"], true);
}

#[test]
fn import_adrs_apply_writes_decisions() {
    let tr = fresh_repo();
    let dir = stage_dir(tr.root(), "adrs", &[("0007.md", ADR_FULL)]);

    let out = run_firetrail(
        tr.root(),
        &["--json", "import", "adrs", dir.to_str().unwrap(), "--apply"],
    );
    assert!(out.success(), "adr import failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["parsed"], 1);
    assert_eq!(v["data"]["written"], 1);
}

#[test]
fn import_runbooks_apply_writes_runbooks() {
    let tr = fresh_repo();
    let dir = stage_dir(tr.root(), "runbooks", &[("rb.md", RUNBOOK_FULL)]);

    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "import",
            "runbooks",
            dir.to_str().unwrap(),
            "--apply",
        ],
    );
    assert!(out.success(), "runbook import failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["parsed"], 1);
    assert_eq!(v["data"]["written"], 1);
}

#[test]
fn search_excludes_quarantined_by_default_and_includes_with_flag() {
    let tr = fresh_repo();
    let dir = stage_dir(tr.root(), "imp-search", &[("a.md", INCIDENT_A)]);
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "import",
            "incidents",
            dir.to_str().unwrap(),
            "--apply",
        ],
    );
    assert!(out.success(), "{}", out.stderr);

    // Default: quarantined records excluded.
    let plain = run_firetrail(
        tr.root(),
        &["--json", "search", "quokka", "--mode", "lexical"],
    );
    assert!(plain.success(), "{}", plain.stderr);
    let v = parse_json(&plain);
    let hits = v["data"]["hits"].as_array().unwrap();
    assert!(
        hits.is_empty(),
        "quarantined hits leaked into default search: {hits:?}"
    );

    // --include-quarantine: surfaces them with marker.
    let inc = run_firetrail(
        tr.root(),
        &[
            "--json",
            "search",
            "quokka",
            "--mode",
            "lexical",
            "--include-quarantine",
        ],
    );
    assert!(inc.success(), "{}", inc.stderr);
    let v = parse_json(&inc);
    let hits = v["data"]["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "expected at least one quarantined hit");
    assert_eq!(hits[0]["quarantine"], true);
}

#[test]
fn prime_excludes_quarantined_by_default() {
    let tr = fresh_repo();
    let dir = stage_dir(tr.root(), "imp-prime", &[("a.md", INCIDENT_A)]);
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "import",
            "incidents",
            dir.to_str().unwrap(),
            "--apply",
        ],
    );
    assert!(out.success(), "{}", out.stderr);

    let plain = run_firetrail(tr.root(), &["--json", "prime", "--query", "quokka"]);
    assert!(plain.success(), "{}", plain.stderr);
    let v = parse_json(&plain);
    let items = v["data"]["items"].as_array().cloned().unwrap_or_default();
    for item in &items {
        // None of the items should be a quarantined import when the flag is
        // unset.
        assert!(
            !item
                .get("quarantine")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            "quarantine leaked into default prime: {item:?}"
        );
    }
}

#[test]
fn promote_import_listing_then_targeted_promote() {
    let tr = fresh_repo();
    let dir = stage_dir(tr.root(), "imp-promote", &[("a.md", INCIDENT_A)]);
    let out = run_firetrail(
        tr.root(),
        &[
            "--json",
            "import",
            "incidents",
            dir.to_str().unwrap(),
            "--apply",
        ],
    );
    assert!(out.success(), "{}", out.stderr);
    let v = parse_json(&out);
    let incident_id = v["data"]["records"][0].as_str().unwrap().to_string();

    // 3 canonical findings referencing the imported incident.
    for _ in 0..3 {
        let f = run_firetrail(
            tr.root(),
            &[
                "--json",
                "finding",
                "create",
                "ref",
                "--incident",
                &incident_id,
                "--details",
                "x",
            ],
        );
        assert!(f.success(), "{}", f.stderr);
    }

    let list = run_firetrail(tr.root(), &["--json", "promote-import"]);
    assert!(list.success(), "{}", list.stderr);
    let v = parse_json(&list);
    let cands = v["data"]["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 1, "expected exactly one candidate");
    assert_eq!(cands[0]["id"].as_str().unwrap(), incident_id);

    let prom = run_firetrail(tr.root(), &["--json", "promote-import", &incident_id]);
    assert!(prom.success(), "{}", prom.stderr);
    let v = parse_json(&prom);
    assert_eq!(v["data"]["action"], "promote");
    assert_eq!(v["data"]["promoted_ids"][0].as_str().unwrap(), incident_id);

    // After promotion the record is no longer a candidate.
    let list2 = run_firetrail(tr.root(), &["--json", "promote-import"]);
    let v = parse_json(&list2);
    assert!(v["data"]["candidates"].as_array().unwrap().is_empty());
}

#[test]
fn jira_import_stub_returns_user_error() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["--json", "jira", "import", "ENG-1"]);
    assert!(!out.success(), "expected stub failure");
    let v: serde_json::Value = serde_json::from_str(out.stderr.trim()).unwrap();
    assert_eq!(v["error"]["kind"].as_str().unwrap(), "user_error");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("jira"),
        "stub error message should mention jira: {}",
        v["error"]["message"]
    );
}

#[test]
fn confluence_import_stub_returns_user_error() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["--json", "import", "confluence", "ENG/123"]);
    assert!(!out.success(), "expected stub failure");
    let v: serde_json::Value = serde_json::from_str(out.stderr.trim()).unwrap();
    assert_eq!(v["error"]["kind"].as_str().unwrap(), "user_error");
}
