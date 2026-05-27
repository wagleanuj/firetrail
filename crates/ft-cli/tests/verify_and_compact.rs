//! `firetrail verify` (incl. force-push / tamper detection) and `firetrail compact`.

mod common;

use common::{fresh_repo, parse_json, run_firetrail};

#[test]
fn verify_clean_repo_passes_with_all_records() {
    let tr = fresh_repo();
    let _ = run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "t1", "--body", "x"],
    );
    let _ = run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "t2", "--body", "y"],
    );
    let out = run_firetrail(tr.root(), &["--json", "verify"]);
    assert!(out.success(), "verify failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["failures"].as_u64(), Some(0));
    assert!(v["data"]["total"].as_u64().unwrap() >= 2);
}

#[test]
fn verify_detects_tampered_state_hash_on_disk() {
    let tr = fresh_repo();
    let create_out = run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "tamper", "--body", "x"],
    );
    let id = parse_json(&create_out)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_lowercase();
    // Locate the on-disk file (kind subdir is `memory`).
    let path = tr
        .root()
        .join(".firetrail/records/memory")
        .join(format!("{id}.json"));
    assert!(path.exists(), "expected record file at {}", path.display());

    // Tamper: rewrite the title without updating state_hash.
    let bytes = std::fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["envelope"]["title"] = serde_json::json!("tampered title");
    std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

    // verify must fail with exit code 1 and surface the tampered id.
    let out = run_firetrail(tr.root(), &["--json", "verify"]);
    assert!(!out.success(), "tampered verify should fail");
    assert_eq!(out.status.code(), Some(1));
    // The JSON error envelope carries the structured report under `details`.
    let env: serde_json::Value = serde_json::from_str(out.stderr.trim()).unwrap_or_else(|e| {
        panic!(
            "stderr not JSON: {e}\nstdout={}\nstderr={}",
            out.stdout, out.stderr
        )
    });
    assert!(env["error"]["details"]["failures"].as_u64().unwrap_or(0) >= 1);
}

#[test]
fn compact_single_record_returns_report() {
    let tr = fresh_repo();
    let out = run_firetrail(
        tr.root(),
        &["--json", "memory", "create", "c1", "--body", "x"],
    );
    let id = parse_json(&out)["data"]["record"]["envelope"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let comp = run_firetrail(tr.root(), &["--json", "compact", &id]);
    assert!(comp.success(), "compact failed: {}", comp.stderr);
    let v = parse_json(&comp);
    let reports = v["data"]["reports"].as_array().cloned().unwrap_or_default();
    assert_eq!(reports.len(), 1);
}
