//! PR-time compaction (ADR-0003).
//!
//! Compaction collapses runs of consecutive `Update` entries authored by
//! the same identity within a configurable window into a single combined
//! entry. Every other kind is audit-critical and preserved verbatim.
//!
//! After squashing, the chain is relinked: each surviving entry's
//! `from_hash` is set to the prior survivor's `to_hash`, the envelope's
//! `state_hash` / `prev_state_hash` are recomputed via the same
//! "open-tail" trick used by [`crate::append_history`], and
//! [`crate::verify_chain`] continues to pass.
//!
//! The compaction is **lossy by design** — it discards intermediate
//! `to_hash` values. ADR-0003 accepts this loss in exchange for bounded
//! record size.

use chrono::Duration;
use ft_core::{HistoryEntry, Record};

use crate::HistoryEntryKind;
use crate::append::{canonical_state_hash_with_open_tail, entry_kind};

/// Compaction policy knobs. See [`Self::default`] for the M2 defaults.
#[derive(Debug, Clone)]
pub struct CompactPolicy {
    /// Maximum time delta between two consecutive squashable entries for
    /// them to be merged. Pairs further apart in wall-clock time are kept
    /// distinct even if author and kind match.
    pub squash_updates_within: Duration,

    /// Kinds preserved verbatim. Defaults to every kind except `Update`.
    /// Override to be more aggressive (e.g. squash `Reopen+Update` pairs)
    /// only with great care.
    pub preserve_kinds: Vec<HistoryEntryKind>,

    /// Optional cap on the final `history[]` length. When `Some(n)` and
    /// the chain still exceeds `n` after kind-based squashing, the oldest
    /// non-preserved entries are merged into a single bucket until the
    /// cap is met. `None` means unlimited.
    pub max_history_len: Option<usize>,
}

impl Default for CompactPolicy {
    fn default() -> Self {
        Self {
            // Default window: 1 hour. Matches ADR-0003's "a series of
            // saves on a working branch" expectation.
            squash_updates_within: Duration::hours(1),
            preserve_kinds: vec![
                HistoryEntryKind::Create,
                HistoryEntryKind::Close,
                HistoryEntryKind::TrustTransition,
                HistoryEntryKind::Supersede,
                HistoryEntryKind::Deprecate,
                HistoryEntryKind::Archive,
                HistoryEntryKind::Redact,
                HistoryEntryKind::Reopen,
            ],
            max_history_len: None,
        }
    }
}

/// Reasons a particular entry was dropped during compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactedKind {
    /// Squashed into a neighbouring `Update` entry by the same author.
    SquashedUpdate,
    /// Removed by the `max_history_len` overflow bucket.
    OverflowBucketed,
}

/// Summary of a compaction run.
#[derive(Debug, Clone, Default)]
pub struct CompactReport {
    /// `history[]` length before compaction.
    pub entries_before: usize,
    /// `history[]` length after compaction.
    pub entries_after: usize,
    /// Per-dropped-entry reasons.
    pub dropped: Vec<CompactedKind>,
}

/// Compact `record.envelope.history` in place under `policy`.
///
/// On return the chain is fully relinked and `state_hash` / `prev_state_hash`
/// reflect the new layout. [`crate::verify_chain`] passes if the input
/// chain was valid.
///
/// # Errors
///
/// Returns `Err` only on a canonical-hashing failure (extremely rare).
pub fn compact_history(
    record: &mut Record,
    policy: &CompactPolicy,
) -> Result<CompactReport, ft_core::CoreError> {
    let before = record.envelope.history.len();
    let mut dropped: Vec<CompactedKind> = Vec::new();

    if before <= 1 {
        return Ok(CompactReport {
            entries_before: before,
            entries_after: before,
            dropped,
        });
    }

    // Phase 1: squash runs of consecutive Update entries by the same actor
    // within `squash_updates_within`.
    let mut compacted: Vec<HistoryEntry> = Vec::with_capacity(before);
    for entry in record.envelope.history.drain(..) {
        let kind = entry_kind(&entry);
        let is_squashable = matches!(kind, Some(HistoryEntryKind::Update))
            && !policy.preserve_kinds.contains(&HistoryEntryKind::Update);

        if is_squashable {
            if let Some(prev) = compacted.last_mut() {
                let prev_kind = entry_kind(prev);
                if prev_kind == Some(HistoryEntryKind::Update)
                    && prev.primary_actor == entry.primary_actor
                    && entry.timestamp.signed_duration_since(prev.timestamp)
                        <= policy.squash_updates_within
                {
                    merge_into(prev, entry);
                    dropped.push(CompactedKind::SquashedUpdate);
                    continue;
                }
            }
        }
        compacted.push(entry);
    }

    // Phase 2: max_history_len overflow bucket. Only non-preserved entries
    // are eligible for bucketing. The bucket replaces the contiguous
    // oldest-eligible run.
    if let Some(cap) = policy.max_history_len {
        while compacted.len() > cap {
            // Find the index of the oldest pair of eligible adjacent
            // entries to merge.
            let merge_at = compacted.iter().position(|e| {
                entry_kind(e).is_some_and(|k| {
                    !policy.preserve_kinds.contains(&k) || k == HistoryEntryKind::Update
                })
            });
            let Some(idx) = merge_at else { break };
            if idx + 1 >= compacted.len() {
                break;
            }
            let removed = compacted.remove(idx + 1);
            merge_into(&mut compacted[idx], removed);
            dropped.push(CompactedKind::OverflowBucketed);
        }
    }

    record.envelope.history = compacted;

    // Phase 3: relink the chain.
    relink_chain(record)?;

    let after = record.envelope.history.len();
    Ok(CompactReport {
        entries_before: before,
        entries_after: after,
        dropped,
    })
}

/// Merge `src` into `dst`, preserving `dst`'s `from_hash` (the run's first
/// entry) and adopting `src`'s `to_hash` / `timestamp` / `merged_via_pr`.
/// Contributors and `ops_summary` are unioned; `ops_count` is summed.
fn merge_into(dst: &mut HistoryEntry, src: HistoryEntry) {
    // Take the later timestamp.
    dst.timestamp = dst.timestamp.max(src.timestamp);
    // Adopt the later PR id if any.
    if src.merged_via_pr.is_some() {
        dst.merged_via_pr = src.merged_via_pr;
    }
    // Union contributors, preserving order and deduplicating.
    for c in src.contributors {
        if !dst.contributors.contains(&c) && c != dst.primary_actor {
            dst.contributors.push(c);
        }
    }
    // Concatenate ops_summary lines (kind tag stays on dst's first line).
    for line in src.ops_summary {
        if !dst.ops_summary.contains(&line) {
            dst.ops_summary.push(line);
        }
    }
    // Sum ops_count.
    dst.ops_count = dst.ops_count.saturating_add(src.ops_count);
    // Adopt the new to_hash (it will be overwritten by relink_chain).
    dst.to_hash = src.to_hash;
}

/// Walk the (now-shortened) history and rewrite `from_hash` / `to_hash` so
/// the chain reads end-to-end again. The body of each entry is preserved.
///
/// Public so the JSON merge driver can re-stitch the chain after a
/// three-way union of `history[]` entries.
pub fn relink_chain(record: &mut Record) -> Result<(), ft_core::CoreError> {
    if record.envelope.history.is_empty() {
        // Hash the body, leave prev as-is (None).
        let h = ft_core::state_hash(record)?;
        record.envelope.state_hash = h;
        record.envelope.prev_state_hash = None;
        return Ok(());
    }

    // First entry is genesis: from_hash = "".
    record.envelope.history[0].from_hash.clear();

    // Iteratively pin each entry's hashes. We must do this from the head
    // forward because each `to_hash` depends on the whole entry contents
    // including its `from_hash`.
    for i in 0..record.envelope.history.len() {
        let from = if i == 0 {
            String::new()
        } else {
            record.envelope.history[i - 1].to_hash.clone()
        };
        record.envelope.history[i].from_hash = from;
        // Clear this entry's to_hash temporarily so the recompute below
        // sees an "open tail" that ends at index i. To do that with the
        // open-tail helper we must temporarily truncate the chain.
        // Cheaper: clone the record, truncate to [0..=i], clear tail, hash.
        let mut tmp = record.clone();
        tmp.envelope.history.truncate(i + 1);
        tmp.envelope
            .history
            .last_mut()
            .expect("non-empty")
            .to_hash
            .clear();
        // We must also clear the envelope hash fields so the canonical
        // hash isn't influenced by stale values. ft_core::state_hash
        // already elides them, but be explicit.
        tmp.envelope.state_hash.clear();
        tmp.envelope.prev_state_hash = None;
        let h = canonical_state_hash_with_open_tail(&tmp).map_err(|e| match e {
            crate::HistoryError::Core(c) => c,
            crate::HistoryError::InvalidDraft(s) => {
                ft_core::CoreError::Serde(serde::de::Error::custom(s))
            }
        })?;
        record.envelope.history[i].to_hash = h;
    }

    // Envelope chain pointer reflects the tail's from_hash.
    let prev = {
        let tail = record
            .envelope
            .history
            .last()
            .expect("non-empty checked above");
        if tail.from_hash.is_empty() {
            None
        } else {
            Some(tail.from_hash.clone())
        }
    };
    record.envelope.prev_state_hash = prev;
    // Closed-form envelope state_hash.
    record.envelope.state_hash.clear();
    let h = ft_core::state_hash(record)?;
    record.envelope.state_hash = h;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HistoryDraft, HistoryEntryKind, append_history, verify_chain};
    use chrono::{TimeZone, Utc};
    use ft_testkit::{make_identity, make_identity_named, make_task};

    fn draft_at(kind: HistoryEntryKind, summary: &str, secs: i64) -> HistoryDraft {
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

    fn draft_by(kind: HistoryEntryKind, summary: &str, secs: i64, who: &str) -> HistoryDraft {
        HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc.timestamp_opt(secs, 0).single().unwrap(),
            primary_actor: make_identity_named(who),
            contributors: Vec::new(),
            ops_summary: vec![summary.to_string()],
            ops_count: 1,
            kind,
        }
    }

    #[test]
    fn no_op_on_short_history() {
        let mut r = make_task().build();
        let report = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        assert_eq!(report.entries_before, 0);
        assert_eq!(report.entries_after, 0);
    }

    #[test]
    fn squashes_consecutive_updates_by_same_author_within_window() {
        let mut r = make_task().build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        // Three updates, all within an hour, by same author.
        r.envelope.title = "v1".into();
        append_history(&mut r, draft_at(HistoryEntryKind::Update, "u1", 60)).unwrap();
        r.envelope.title = "v2".into();
        append_history(&mut r, draft_at(HistoryEntryKind::Update, "u2", 120)).unwrap();
        r.envelope.title = "v3".into();
        append_history(&mut r, draft_at(HistoryEntryKind::Update, "u3", 180)).unwrap();
        assert_eq!(r.envelope.history.len(), 4);

        let report = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        assert_eq!(report.entries_before, 4);
        // Create + one squashed Update = 2.
        assert_eq!(report.entries_after, 2);
        assert_eq!(report.dropped.len(), 2);
        // Chain still verifies.
        verify_chain(&r).unwrap();
        // Squashed entry kept the update tag.
        let last = r.envelope.history.last().unwrap();
        let head = &last.ops_summary[0];
        assert!(head.starts_with("update:"));
        assert_eq!(last.ops_count, 3);
    }

    #[test]
    fn does_not_squash_across_different_authors() {
        let mut r = make_task().build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        r.envelope.title = "a".into();
        append_history(
            &mut r,
            draft_by(HistoryEntryKind::Update, "u1", 60, "alice"),
        )
        .unwrap();
        r.envelope.title = "b".into();
        append_history(&mut r, draft_by(HistoryEntryKind::Update, "u2", 120, "bob")).unwrap();

        let report = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        assert_eq!(report.entries_before, 3);
        assert_eq!(report.entries_after, 3);
        verify_chain(&r).unwrap();
    }

    #[test]
    fn does_not_squash_outside_window() {
        let mut r = make_task().build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        r.envelope.title = "a".into();
        append_history(&mut r, draft_at(HistoryEntryKind::Update, "u1", 60)).unwrap();
        // Two hours later → outside the default 1-hour window.
        r.envelope.title = "b".into();
        append_history(
            &mut r,
            draft_at(HistoryEntryKind::Update, "u2", 60 + 60 * 60 * 2),
        )
        .unwrap();

        let report = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        assert_eq!(report.entries_before, 3);
        assert_eq!(report.entries_after, 3);
        verify_chain(&r).unwrap();
    }

    #[test]
    fn preserves_trust_transition_between_updates() {
        let mut r = make_task().build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        r.envelope.title = "a".into();
        append_history(&mut r, draft_at(HistoryEntryKind::Update, "u1", 60)).unwrap();
        r.envelope.title = "b".into();
        append_history(
            &mut r,
            draft_at(HistoryEntryKind::TrustTransition, "promote", 120),
        )
        .unwrap();
        r.envelope.title = "c".into();
        append_history(&mut r, draft_at(HistoryEntryKind::Update, "u2", 180)).unwrap();

        let report = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        // TrustTransition splits the two Update entries → no squash possible.
        assert_eq!(report.entries_before, 4);
        assert_eq!(report.entries_after, 4);
        verify_chain(&r).unwrap();
    }

    #[test]
    fn relink_keeps_chain_verifiable_after_squash() {
        let mut r = make_task().build();
        append_history(&mut r, draft_at(HistoryEntryKind::Create, "born", 0)).unwrap();
        for i in 0..5 {
            r.envelope.title = format!("v{i}");
            append_history(
                &mut r,
                draft_at(HistoryEntryKind::Update, &format!("u{i}"), 60 + i * 60),
            )
            .unwrap();
        }
        let _ = compact_history(&mut r, &CompactPolicy::default()).unwrap();
        verify_chain(&r).expect("chain must verify after squash");
    }
}
