//! Integration tests for ft-history: append → write → read → verify
//! round-trip, plus a tamper test that exercises the on-disk
//! force-push detection surface.

use chrono::{TimeZone, Utc};
use ft_core::state_hash;
use ft_history::{HistoryDraft, HistoryEntryKind, append_history, verify_chain, verify_repository};
use ft_storage::{EmbeddedStorage, Storage};
use ft_testkit::{TestRepo, make_identity, make_task};

fn draft(kind: HistoryEntryKind, summary: &str, secs: i64) -> HistoryDraft {
    HistoryDraft {
        merged_via_pr: None,
        timestamp: Utc.timestamp_opt(secs, 0).single().unwrap(),
        primary_actor: make_identity(),
        contributors: Vec::new(),
        ops_summary: vec![summary.to_string()],
        ops_count: 1,
        kind,
    }
}

#[test]
fn round_trip_create_mutate_write_read_verify() {
    let tr = TestRepo::new().unwrap();
    let storage = EmbeddedStorage::open(tr.root()).unwrap();
    let mut r = make_task().build();
    append_history(&mut r, draft(HistoryEntryKind::Create, "born", 0)).unwrap();
    r.envelope.title = "v2".into();
    append_history(&mut r, draft(HistoryEntryKind::Update, "retitle", 60)).unwrap();

    storage.write(&r).unwrap();
    let back = storage.read(&r.envelope.id).unwrap();
    assert_eq!(back, r);
    verify_chain(&back).expect("verify must pass after disk round-trip");
}

#[test]
fn verify_repository_walks_all_records() {
    let tr = TestRepo::new().unwrap();
    let storage = EmbeddedStorage::open(tr.root()).unwrap();
    for i in 0..3 {
        let mut r = make_task().title(format!("t{i}")).build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born", i)).unwrap();
        storage.write(&r).unwrap();
    }
    let report = verify_repository(&storage, None);
    assert_eq!(report.total, 3);
    assert_eq!(report.verified, 3);
    assert!(report.is_clean());
}

#[test]
fn on_disk_tamper_surfaces_as_storage_hash_mismatch() {
    // Force-push / corruption analogue: rewrite the JSON file on disk so
    // its body no longer matches the embedded state_hash. ft-storage's
    // own integrity check (which uses ft-core::state_hash) trips first;
    // verify_repository reports the failure.
    let tr = TestRepo::new().unwrap();
    let storage = EmbeddedStorage::open(tr.root()).unwrap();
    let mut r = make_task().title("orig").build();
    append_history(&mut r, draft(HistoryEntryKind::Create, "born", 0)).unwrap();
    storage.write(&r).unwrap();

    // Tamper: rewrite title bytes without updating state_hash.
    let path = storage.path_for(&r.envelope.id);
    let bytes = std::fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["envelope"]["title"] = serde_json::json!("tampered");
    std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

    let report = verify_repository(&storage, None);
    assert_eq!(report.total, 1);
    assert_eq!(report.verified, 0);
    assert_eq!(report.failures.len(), 1);
    let f = &report.failures[0];
    assert!(
        f.reason.contains("hash") || f.reason.contains("storage"),
        "reason should mention hash failure, got: {}",
        f.reason
    );
}

#[test]
fn verify_catches_in_record_chain_tamper() {
    // The storage layer fails fast on body tamper, but the in-record
    // chain tamper (a flipped link inside history[]) is what verify_chain
    // is designed to catch. Build the record in memory, tamper, verify.
    let mut r = make_task().build();
    append_history(&mut r, draft(HistoryEntryKind::Create, "born", 0)).unwrap();
    r.envelope.title = "v2".into();
    append_history(&mut r, draft(HistoryEntryKind::Update, "u1", 60)).unwrap();
    r.envelope.title = "v3".into();
    append_history(&mut r, draft(HistoryEntryKind::Update, "u2", 120)).unwrap();

    // Tamper the middle link.
    r.envelope.history[1].to_hash = "deadbeef".into();
    let err = verify_chain(&r).unwrap_err();
    // Either BrokenLink at idx 2, or a re-hash failure depending on which
    // invariant the verifier hits first; both are acceptable signals.
    let msg = err.to_string();
    assert!(
        msg.contains("broken")
            || msg.contains("Broken")
            || msg.contains("mismatch")
            || msg.contains("desync"),
        "unexpected error: {msg}"
    );
}

#[test]
fn state_hash_canonical_recompute_matches_envelope() {
    // Sanity: after append_history, ft_core::state_hash (the closed-form
    // hash that ft-storage validates) must equal envelope.state_hash.
    let mut r = make_task().build();
    append_history(&mut r, draft(HistoryEntryKind::Create, "born", 0)).unwrap();
    let h = state_hash(&r).unwrap();
    assert_eq!(
        h, r.envelope.state_hash,
        "envelope hash must be closed-form"
    );
    verify_chain(&r).unwrap();
}
