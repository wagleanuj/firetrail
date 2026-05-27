//! Integration tests for ft-history: append → write → read → verify
//! round-trip, plus a tamper test that exercises the on-disk
//! force-push detection surface.

use chrono::{TimeZone, Utc};
use ft_core::state_hash;
use ft_history::{HistoryDraft, HistoryEntryKind, append_history, verify_chain};
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
