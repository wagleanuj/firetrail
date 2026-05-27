//! Append a new entry to a record's history chain.
//!
//! `append_history` is the only sanctioned way to mutate
//! `record.envelope.history` and the two hash fields that track the chain
//! head. It assumes the caller has already applied the desired field-level
//! mutations to `record` *before* calling.
//!
//! The function:
//!
//! 1. Captures the envelope's current `state_hash` as the new entry's
//!    `from_hash` (or the empty string if the record has no prior history —
//!    see [`crate::VerifyError::MissingGenesis`]).
//! 2. Pushes the entry onto `envelope.history` with a placeholder `to_hash`.
//! 3. Re-hashes the record (note: `state_hash` and `prev_state_hash` are
//!    elided from the canonical-JSON form by [`ft_core::state_hash`], so the
//!    placeholder is benign).
//! 4. Writes the new hash back into both the envelope (`state_hash`) and
//!    into the just-pushed entry's `to_hash`.
//! 5. Sets `prev_state_hash` to the entry's `from_hash`.

use chrono::{DateTime, Utc};
use ft_core::{HistoryEntry, Identity, Record, state_hash};

use crate::HistoryEntryKind;
use crate::error::HistoryError;

/// Caller-facing description of a not-yet-linked history entry.
///
/// `HistoryDraft` is intentionally minimal — it describes *what changed*
/// and *who changed it*. `append_history` is responsible for filling in
/// `from_hash` / `to_hash` so the chain is always self-consistent.
#[derive(Debug, Clone)]
pub struct HistoryDraft {
    /// PR number that merged this batch of changes, if known. Compaction
    /// preserves this. Per-mutation appends inside a feature branch may
    /// leave it `None` and let `compact_history` stamp it at merge time.
    pub merged_via_pr: Option<u64>,
    /// Wall-clock time the change occurred.
    pub timestamp: DateTime<Utc>,
    /// Primary actor on the change.
    pub primary_actor: Identity,
    /// Co-authors / reviewers / additional contributors.
    pub contributors: Vec<Identity>,
    /// One-line human-readable operation summaries describing the change.
    /// The first surviving summary always starts with the
    /// [`HistoryEntryKind::as_tag`] of `kind` followed by a colon; callers
    /// supply the suffix here.
    pub ops_summary: Vec<String>,
    /// Total number of underlying mutations the entry compacts. `1` for a
    /// per-mutation append.
    pub ops_count: u32,
    /// Coarse semantic classification (see [`HistoryEntryKind`]).
    pub kind: HistoryEntryKind,
}

impl HistoryDraft {
    /// Encode the draft into an [`ft_core::HistoryEntry`] with placeholder
    /// hash fields. Callers do not normally invoke this directly;
    /// [`append_history`] uses it internally.
    fn into_entry_with_hashes(self, from_hash: String, to_hash: String) -> HistoryEntry {
        // Stamp the kind tag as a prefix on the first ops_summary line so
        // the kind survives the serde round-trip (HistoryEntry has no
        // dedicated kind field).
        let prefix = format!("{}: ", self.kind.as_tag());
        let mut ops_summary = if self.ops_summary.is_empty() {
            vec![format!("{}{}", prefix, "(no summary)")]
        } else {
            let mut v = Vec::with_capacity(self.ops_summary.len());
            let head = self.ops_summary[0].clone();
            // Avoid double-tagging if a caller already prefixed.
            let head = if head.starts_with(&prefix) {
                head
            } else {
                format!("{prefix}{head}")
            };
            v.push(head);
            v.extend(self.ops_summary.into_iter().skip(1));
            v
        };
        // Stable order: keep the kind-tagged line first.
        if ops_summary.is_empty() {
            ops_summary.push(format!("{prefix}(no summary)"));
        }

        HistoryEntry {
            merged_via_pr: self.merged_via_pr,
            timestamp: self.timestamp,
            primary_actor: self.primary_actor,
            contributors: self.contributors,
            ops_summary,
            ops_count: self.ops_count,
            from_hash,
            to_hash,
        }
    }
}

/// Read the [`HistoryEntryKind`] back out of an entry's `ops_summary`.
///
/// Returns `None` if the first summary line is missing or does not begin
/// with a recognized kind tag.
#[must_use]
pub(crate) fn entry_kind(entry: &HistoryEntry) -> Option<HistoryEntryKind> {
    let head = entry.ops_summary.first()?;
    let tag = head.split_once(':').map(|(t, _)| t.trim())?;
    HistoryEntryKind::from_tag(tag)
}

/// Append a history entry to `record`, updating the envelope's chain
/// pointers.
///
/// The caller must have already applied the body-level changes the entry
/// describes. After this call, `record.envelope.state_hash` reflects the
/// new content and `record.envelope.prev_state_hash` reflects the prior
/// chain head. The pushed entry's `to_hash` matches the new `state_hash`.
///
/// # Errors
///
/// - [`HistoryError::InvalidDraft`] if `draft.ops_count == 0`.
/// - [`HistoryError::Core`] if canonical hashing fails.
pub fn append_history(record: &mut Record, draft: HistoryDraft) -> Result<(), HistoryError> {
    if draft.ops_count == 0 {
        return Err(HistoryError::InvalidDraft(
            "ops_count must be >= 1".to_string(),
        ));
    }

    // The prior chain head: empty string for the genesis entry, otherwise
    // the previous tail entry's `to_hash`. We do NOT chain off the
    // envelope's closed-form `state_hash` because that hash includes the
    // entire history[] (and so would change every time we append),
    // whereas `to_hash` on a given entry is fixed once the entry is the
    // tail.
    let from_hash = record
        .envelope
        .history
        .last()
        .map_or_else(String::new, |e| e.to_hash.clone());

    // Push the entry with an empty `to_hash`. Then compute the entry's
    // own `to_hash` as the canonical hash of the record truncated to
    // include only this entry as the tail (with its `to_hash` still
    // empty). This is recomputable in [`crate::verify_chain`].
    let entry = draft.into_entry_with_hashes(from_hash.clone(), String::new());
    record.envelope.history.push(entry);

    // Step 1: derive the entry's `to_hash` via the open-tail hash of the
    // current record (the new entry IS the tail and its to_hash is "").
    let entry_to_hash = canonical_state_hash_with_open_tail(record)?;
    if let Some(tail) = record.envelope.history.last_mut() {
        tail.to_hash.clone_from(&entry_to_hash);
    }

    // Step 2: now write the envelope's chain pointers, clear state_hash
    // so it doesn't leak into the closed-form hash, then compute and
    // assign the closed-form state_hash. (ft_core::state_hash elides
    // state_hash and prev_state_hash from the canonical form, so the
    // order here is for clarity, not correctness.)
    record.envelope.prev_state_hash = if from_hash.is_empty() {
        None
    } else {
        Some(from_hash)
    };
    record.envelope.state_hash.clear();
    let envelope_hash = state_hash(record)?;
    record.envelope.state_hash = envelope_hash;

    Ok(())
}

/// Compute the canonical state hash of `record` with the final
/// `history[]` entry's `to_hash` field treated as the empty string.
///
/// This is the helper used to derive each `history[i].to_hash` value:
/// truncate the chain to `[0..=i]`, clear the tail's `to_hash`, hash the
/// whole record. The result is reproducible in [`crate::verify_chain`]
/// because the truncated form is fully determined by the entry contents
/// and the body.
///
/// `ft_core::state_hash` itself elides `envelope.state_hash` and
/// `envelope.prev_state_hash`, so this function only needs to clear the
/// tail `to_hash`.
pub(crate) fn canonical_state_hash_with_open_tail(record: &Record) -> Result<String, HistoryError> {
    if record.envelope.history.is_empty() {
        return Ok(state_hash(record)?);
    }
    // Clone the record into a temporary with the tail to_hash cleared.
    let mut tmp = record.clone();
    if let Some(tail) = tmp.envelope.history.last_mut() {
        tail.to_hash.clear();
    }
    Ok(state_hash(&tmp)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ft_testkit::{make_identity, make_task};

    fn draft(kind: HistoryEntryKind, summary: &str, when_secs: i64) -> HistoryDraft {
        HistoryDraft {
            merged_via_pr: None,
            timestamp: Utc.timestamp_opt(when_secs, 0).single().unwrap(),
            primary_actor: make_identity(),
            contributors: Vec::new(),
            ops_summary: vec![summary.to_string()],
            ops_count: 1,
            kind,
        }
    }

    #[test]
    fn append_genesis_sets_from_hash_empty_and_prev_none() {
        let mut r = make_task().build();
        // Builder produced a state_hash but no history yet.
        assert!(r.envelope.history.is_empty());
        append_history(&mut r, draft(HistoryEntryKind::Create, "born", 1)).unwrap();
        let tail = r.envelope.history.last().unwrap();
        assert_eq!(tail.from_hash, "");
        // The tail to_hash is the open-tail hash; the envelope hash is
        // the closed-form hash. They are deliberately different — see
        // [`canonical_state_hash_with_open_tail`].
        assert!(!tail.to_hash.is_empty());
        assert_ne!(tail.to_hash, r.envelope.state_hash);
        // Envelope hash matches the closed-form ft_core::state_hash.
        assert_eq!(r.envelope.state_hash, ft_core::state_hash(&r).unwrap());
        assert_eq!(r.envelope.prev_state_hash, None);
        assert_eq!(entry_kind(tail), Some(HistoryEntryKind::Create));
    }

    #[test]
    fn append_links_subsequent_entry_to_prior_to_hash() {
        let mut r = make_task().build();
        append_history(&mut r, draft(HistoryEntryKind::Create, "born", 1)).unwrap();
        let to_after_1 = r.envelope.history[0].to_hash.clone();
        let env_after_1 = r.envelope.state_hash.clone();
        // The envelope hash is the closed-form ft_core hash of the
        // current record state.
        assert_eq!(env_after_1, ft_core::state_hash(&r).unwrap());
        // Mutate, then append.
        r.envelope.title = "renamed".into();
        append_history(&mut r, draft(HistoryEntryKind::Update, "retitle", 2)).unwrap();
        let entries = &r.envelope.history;
        assert_eq!(entries.len(), 2);
        // Chain link: entry[1].from_hash == entry[0].to_hash.
        assert_eq!(entries[1].from_hash, to_after_1);
        // Envelope's prev_state_hash mirrors the new tail's from_hash.
        assert_eq!(
            r.envelope.prev_state_hash.as_deref(),
            Some(to_after_1.as_str())
        );
        // Envelope state_hash changed because the body changed.
        assert_ne!(r.envelope.state_hash, env_after_1);
        // Envelope hash matches the closed-form ft_core::state_hash.
        assert_eq!(r.envelope.state_hash, ft_core::state_hash(&r).unwrap());
    }

    #[test]
    fn append_rejects_zero_ops_count() {
        let mut r = make_task().build();
        let mut d = draft(HistoryEntryKind::Update, "x", 1);
        d.ops_count = 0;
        let err = append_history(&mut r, d).unwrap_err();
        assert!(matches!(err, HistoryError::InvalidDraft(_)));
    }

    #[test]
    fn ops_summary_prefix_is_idempotent() {
        let mut r = make_task().build();
        let mut d = draft(HistoryEntryKind::Create, "create: born", 1);
        d.kind = HistoryEntryKind::Create;
        append_history(&mut r, d).unwrap();
        assert_eq!(r.envelope.history[0].ops_summary[0], "create: born");
    }
}
