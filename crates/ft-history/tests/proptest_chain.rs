//! Property tests for the append → verify round-trip.

use chrono::{TimeZone, Utc};
use ft_history::{
    CompactPolicy, HistoryDraft, HistoryEntryKind, append_history, compact_history, verify_chain,
};
use ft_testkit::{make_identity, make_identity_named, make_task};
use proptest::prelude::*;

fn kind_strategy() -> impl Strategy<Value = HistoryEntryKind> {
    prop_oneof![
        Just(HistoryEntryKind::Update),
        Just(HistoryEntryKind::TrustTransition),
        Just(HistoryEntryKind::Close),
        Just(HistoryEntryKind::Reopen),
        Just(HistoryEntryKind::Supersede),
        Just(HistoryEntryKind::Deprecate),
        Just(HistoryEntryKind::Archive),
        Just(HistoryEntryKind::Redact),
    ]
}

#[derive(Debug, Clone)]
struct Op {
    kind: HistoryEntryKind,
    actor: String,
    delta_secs: i64,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    (
        kind_strategy(),
        prop_oneof![Just("alice".to_string()), Just("bob".to_string())],
        1i64..7200i64,
    )
        .prop_map(|(kind, actor, delta_secs)| Op {
            kind,
            actor,
            delta_secs,
        })
}

proptest! {
    /// For any sequence of legal appends, verify_chain succeeds.
    #[test]
    fn append_then_verify_always_passes(ops in proptest::collection::vec(op_strategy(), 0..16)) {
        let mut r = make_task().build();
        // Genesis Create.
        let genesis = HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc.timestamp_opt(0, 0).single().unwrap(),
            primary_actor: make_identity(),
            contributors: Vec::new(),
            ops_summary: vec!["genesis".to_string()],
            ops_count: 1,
            kind: HistoryEntryKind::Create,
        };
        append_history(&mut r, genesis).unwrap();

        let mut t: i64 = 1;
        for (i, op) in ops.iter().enumerate() {
            t = t.saturating_add(op.delta_secs);
            // Mutate body so the entry has a real effect.
            r.envelope.title = format!("t-{i}");
            let d = HistoryDraft {
                merged_via_pr: None,
                timestamp: Utc.timestamp_opt(t, 0).single().unwrap(),
                primary_actor: make_identity_named(&op.actor),
                contributors: Vec::new(),
                ops_summary: vec![format!("op-{i}")],
                ops_count: 1,
                kind: op.kind,
            };
            append_history(&mut r, d).unwrap();
        }
        prop_assert!(verify_chain(&r).is_ok(), "verify_chain failed: {:?}", verify_chain(&r));
    }

    /// Compaction preserves verifiability for any input sequence.
    #[test]
    fn compact_preserves_verifiability(ops in proptest::collection::vec(op_strategy(), 0..12)) {
        let mut r = make_task().build();
        let genesis = HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc.timestamp_opt(0, 0).single().unwrap(),
            primary_actor: make_identity(),
            contributors: Vec::new(),
            ops_summary: vec!["genesis".to_string()],
            ops_count: 1,
            kind: HistoryEntryKind::Create,
        };
        append_history(&mut r, genesis).unwrap();

        let mut t: i64 = 1;
        for (i, op) in ops.iter().enumerate() {
            t = t.saturating_add(op.delta_secs);
            r.envelope.title = format!("t-{i}");
            let d = HistoryDraft {
                merged_via_pr: None,
                timestamp: Utc.timestamp_opt(t, 0).single().unwrap(),
                primary_actor: make_identity_named(&op.actor),
                contributors: Vec::new(),
                ops_summary: vec![format!("op-{i}")],
                ops_count: 1,
                kind: op.kind,
            };
            append_history(&mut r, d).unwrap();
        }

        let report = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        prop_assert!(report.entries_after <= report.entries_before);
        prop_assert!(verify_chain(&r).is_ok(), "verify_chain failed after compact");
    }
}
