//! Chain verification — per-record and per-repository.
//!
//! The chain shape, by construction in [`crate::append_history`]:
//!
//! ```text
//! envelope.history[0].from_hash == ""        (genesis)
//! envelope.history[i].from_hash == envelope.history[i-1].to_hash   for i > 0
//! envelope.history[i].to_hash   == canonical_hash(record_truncated_to(0..=i)
//!                                                 with that tail's to_hash="")
//! envelope.state_hash            == ft_core::state_hash(record)   (closed form)
//! envelope.prev_state_hash       == envelope.history.last().from_hash  (or None at genesis)
//! ```
//!
//! The closed-form envelope `state_hash` is what `ft-storage` validates
//! on write, so records produced by [`crate::append_history`] are safe to
//! persist. Each entry's `to_hash` is independently recomputable in
//! [`verify_chain`] by truncating the chain and re-hashing.

use ft_core::Record;

use crate::append::canonical_state_hash_with_open_tail;
use crate::error::VerifyError;

/// Verify the `prev_state_hash` chain on a single record.
///
/// On success, returns `Ok(())`. On failure, returns the first integrity
/// violation encountered (verification is short-circuit; callers that want
/// a full diagnosis should re-verify after a fix).
///
/// Records with an empty `history[]` are accepted only if they also have
/// no `prev_state_hash` and their `state_hash` matches a recomputed hash
/// of the body. This corresponds to the M1-on-disk shape where
/// `ft-history` has not yet been invoked.
pub fn verify_chain(record: &Record) -> Result<(), VerifyError> {
    let env = &record.envelope;

    if env.history.is_empty() {
        // No chain: tolerate iff there is no prev pointer and the stored
        // state_hash agrees with the body.
        if env.prev_state_hash.is_some() {
            return Err(VerifyError::EmptyHistory);
        }
        let computed = ft_core::state_hash(record)?;
        if computed != env.state_hash {
            return Err(VerifyError::HashMismatch {
                stored: env.state_hash.clone(),
                computed,
                at: "envelope".to_string(),
            });
        }
        return Ok(());
    }

    // First entry must be a genesis link (from_hash empty).
    let first = &env.history[0];
    if !first.from_hash.is_empty() {
        return Err(VerifyError::MissingGenesis {
            got: first.from_hash.clone(),
        });
    }

    // Successive entries must chain.
    for i in 1..env.history.len() {
        let prior_to = &env.history[i - 1].to_hash;
        let this_from = &env.history[i].from_hash;
        if this_from != prior_to {
            return Err(VerifyError::BrokenLink {
                at_index: i,
                from_hash: this_from.clone(),
                prior_to_hash: prior_to.clone(),
            });
        }
    }

    let tail = env.history.last().expect("non-empty checked above");

    // prev_state_hash on the envelope must reflect the tail.from_hash.
    let expected_prev = if tail.from_hash.is_empty() {
        None
    } else {
        Some(tail.from_hash.clone())
    };
    if env.prev_state_hash != expected_prev {
        return Err(VerifyError::EnvelopeChainDesync {
            envelope: env
                .prev_state_hash
                .clone()
                .unwrap_or_else(|| "<none>".to_string()),
            tail: tail.from_hash.clone(),
        });
    }

    // The envelope's state_hash must be the closed-form ft_core hash of
    // the record as-is.
    let envelope_recompute = ft_core::state_hash(record)?;
    if envelope_recompute != env.state_hash {
        return Err(VerifyError::HashMismatch {
            stored: env.state_hash.clone(),
            computed: envelope_recompute,
            at: "envelope".to_string(),
        });
    }

    // The tail entry's `to_hash` is the open-tail hash of the full
    // current record. This is the strongest tamper check we can perform
    // against the current body — intermediate entries' `to_hash` values
    // are historical witnesses (the prior body state is not recoverable
    // from the current snapshot) and are checked only via the chain
    // links above.
    let tail_recompute = canonical_state_hash_with_open_tail(record)
        .map_err(|e| VerifyError::Core(e.to_string()))?;
    if tail_recompute != tail.to_hash {
        return Err(VerifyError::HashMismatch {
            stored: tail.to_hash.clone(),
            computed: tail_recompute,
            at: format!("history[{}]", env.history.len() - 1),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HistoryDraft, HistoryEntryKind, append_history};
    use chrono::Utc;
    use ft_testkit::{make_identity, make_task};

    fn draft(kind: HistoryEntryKind, summary: &str) -> HistoryDraft {
        HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc::now(),
            primary_actor: make_identity(),
            contributors: Vec::new(),
            ops_summary: vec![summary.to_string()],
            ops_count: 1,
            kind,
            transition: None,
        }
    }

    #[test]
    fn empty_history_passes_when_envelope_consistent() {
        let r = make_task().build();
        verify_chain(&r).unwrap();
    }

    #[test]
    fn single_genesis_entry_passes() {
        let mut r = make_task().build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        verify_chain(&r).unwrap();
    }

    #[test]
    fn multi_entry_chain_passes() {
        let mut r = make_task().build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        r.envelope.title = "v2".into();
        append_history(&mut r, draft(HistoryEntryKind::Update, "rename")).unwrap();
        r.envelope.title = "v3".into();
        append_history(&mut r, draft(HistoryEntryKind::Update, "rename2")).unwrap();
        verify_chain(&r).unwrap();
    }

    #[test]
    fn detects_missing_genesis() {
        let mut r = make_task().build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        // Corrupt the first entry's from_hash.
        r.envelope.history[0].from_hash = "deadbeef".into();
        let err = verify_chain(&r).unwrap_err();
        assert!(matches!(err, VerifyError::MissingGenesis { .. }));
    }

    #[test]
    fn detects_broken_link() {
        let mut r = make_task().build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        r.envelope.title = "v2".into();
        append_history(&mut r, draft(HistoryEntryKind::Update, "rename")).unwrap();
        // Tamper: break the link between [0] and [1].
        r.envelope.history[1].from_hash = "deadbeef".into();
        let err = verify_chain(&r).unwrap_err();
        assert!(matches!(err, VerifyError::BrokenLink { at_index: 1, .. }));
    }

    #[test]
    fn detects_hash_mismatch_after_body_tamper() {
        let mut r = make_task().build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
        // Body tamper that leaves the chain links pointing at the old
        // hash. Verifier must recompute and catch it.
        r.envelope.title = "tampered".into();
        let err = verify_chain(&r).unwrap_err();
        assert!(matches!(err, VerifyError::HashMismatch { .. }));
    }

    #[test]
    fn detects_empty_history_with_dangling_prev() {
        let mut r = make_task().build();
        r.envelope.prev_state_hash = Some("dangling".into());
        // history is empty but prev is Some.
        let err = verify_chain(&r).unwrap_err();
        assert_eq!(err, VerifyError::EmptyHistory);
    }
}
