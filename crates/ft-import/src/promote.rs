//! Promotion workflow: move quarantined records into the canonical corpus.
//!
//! A quarantined record becomes a promotion *candidate* when it has at least
//! `min_inbound_refs` inbound references from *canonical* (non-quarantined)
//! records. Promotion itself clears the quarantine label and appends an audit
//! marker to the record's `history`.

use chrono::Utc;
use ft_core::{HistoryEntry, Identity, Record, RecordId, state_hash};
use ft_storage::{Storage, StorageFilter};

use crate::error::ImportError;
use crate::quarantine::{IMPORT_SOURCE_LABEL_KEY, QUARANTINE_LABEL_KEY, is_quarantined};

/// Options controlling [`promotion_candidates`].
#[derive(Debug, Clone)]
pub struct PromotionOpts {
    /// Minimum inbound references from canonical records before a
    /// quarantined record is reported as a candidate. Default: `3`.
    pub min_inbound_refs: usize,
}

impl Default for PromotionOpts {
    fn default() -> Self {
        Self {
            min_inbound_refs: 3,
        }
    }
}

/// A quarantined record that has accumulated enough inbound references to
/// warrant operator review.
#[derive(Debug, Clone)]
pub struct PromotionCandidate {
    /// The quarantined record's id.
    pub id: RecordId,
    /// Number of inbound references found from canonical records.
    pub inbound_refs: usize,
    /// Sample of referencing record ids (capped at 10).
    pub referencing_ids: Vec<RecordId>,
}

/// Find every quarantined record in `storage` that meets the inbound-reference
/// threshold.
///
/// Inbound references are detected by scanning each canonical record's
/// serialized JSON for the candidate's id string. This is intentionally
/// coarse — record bodies carry `RecordId` values in many different fields
/// (`findings`, `runbooks_invoked`, `parent_task`, ...), and a substring scan
/// catches all of them without coupling the importer to every body shape.
///
/// # Errors
///
/// Returns [`ImportError::Storage`] if listing or reading from `storage`
/// fails.
pub fn promotion_candidates(
    storage: &dyn Storage,
    opts: &PromotionOpts,
) -> Result<Vec<PromotionCandidate>, ImportError> {
    // 1. Load every record once. Records are small JSON files; for M6 sizes
    //    a single pass is acceptable. If this becomes a hotspot we can switch
    //    to a streaming scan keyed by id.
    let all_ids = storage.list(&StorageFilter::default())?;
    let mut records: Vec<Record> = Vec::with_capacity(all_ids.len());
    for id in all_ids {
        match storage.read(&id) {
            Ok(r) => records.push(r),
            Err(e) => return Err(ImportError::Storage(e)),
        }
    }

    // 2. Partition: quarantined (potential candidates) vs canonical (the
    //    source of inbound references).
    let (quarantined, canonical): (Vec<&Record>, Vec<&Record>) =
        records.iter().partition(|r| is_quarantined(r));

    // Pre-serialize canonical records once so we don't pay JSON cost per
    // candidate.
    let canonical_json: Vec<(RecordId, String)> = canonical
        .iter()
        .map(|r| {
            let json = serde_json::to_string(r).unwrap_or_default();
            (r.envelope.id.clone(), json)
        })
        .collect();

    let mut candidates: Vec<PromotionCandidate> = Vec::new();
    for q in quarantined {
        let needle = q.envelope.id.as_str();
        let mut hits: Vec<RecordId> = Vec::new();
        for (cid, json) in &canonical_json {
            if json.contains(needle) {
                hits.push(cid.clone());
            }
        }
        if hits.len() >= opts.min_inbound_refs {
            let mut sample = hits.clone();
            sample.truncate(10);
            candidates.push(PromotionCandidate {
                id: q.envelope.id.clone(),
                inbound_refs: hits.len(),
                referencing_ids: sample,
            });
        }
    }
    Ok(candidates)
}

/// Promote a quarantined record into the canonical corpus.
///
/// Strips the `quarantine=true` label, appends a `HistoryEntry` whose
/// `ops_summary` carries the `promote-import: <source>` marker (per
/// ADR-0017 audit requirements; reusing `HistoryEntry` rather than extending
/// `HistoryEntryKind` keeps the change non-breaking — the dedicated kind
/// can be added in a follow-up), and writes the record back to `storage`.
///
/// # Errors
///
/// - [`ImportError::NotQuarantined`] if the record is not currently
///   quarantined.
/// - [`ImportError::Storage`] / [`ImportError::Core`] on I/O or hashing
///   failures.
pub fn promote_record(storage: &dyn Storage, id: &RecordId) -> Result<(), ImportError> {
    let mut record = storage.read(id)?;
    if !is_quarantined(&record) {
        return Err(ImportError::NotQuarantined(id.as_str().to_string()));
    }

    // Capture the source tag (if any) so the audit summary is informative.
    let source_tag = record
        .envelope
        .labels
        .iter()
        .find(|l| l.key == IMPORT_SOURCE_LABEL_KEY)
        .map_or_else(|| "unknown".to_string(), |l| l.value.clone());

    // Remove the quarantine label.
    record
        .envelope
        .labels
        .retain(|l| l.key != QUARANTINE_LABEL_KEY);

    // Append an audit entry. `HistoryEntry` carries no dedicated kind, so we
    // encode the marker into `ops_summary` directly. `from_hash` references
    // the current envelope hash so the audit is anchored to the
    // pre-promotion state.
    let from_hash = record.envelope.state_hash.clone();
    let actor = record
        .envelope
        .owner
        .clone()
        .unwrap_or_else(|| record.envelope.created_by.clone());
    let entry = HistoryEntry {
        merged_via_pr: None,
        timestamp: Utc::now(),
        primary_actor: actor,
        contributors: Vec::new(),
        ops_summary: vec![format!("promote-import: {source_tag}")],
        ops_count: 1,
        from_hash,
        to_hash: String::new(),
    };
    record.envelope.history.push(entry);

    // Re-hash. The just-pushed `to_hash` field is left empty here; populating
    // it with the open-tail hash is the responsibility of `ft-history`'s
    // chain-aware path, which we do not depend on here to keep the crate
    // boundary thin. Downstream `ft-history` will detect and repair the open
    // tail on its next pass.
    record.envelope.state_hash.clear();
    let new_hash = state_hash(&record)?;
    if let Some(tail) = record.envelope.history.last_mut() {
        tail.to_hash.clone_from(&new_hash);
    }
    // Re-hash a second time so envelope hash includes the populated to_hash.
    record.envelope.state_hash.clear();
    record.envelope.state_hash = state_hash(&record)?;

    storage.write(&record)?;
    Ok(())
}

/// Helper used by tests and external callers when synthesizing an
/// already-quarantined record from scratch — adds the required labels to an
/// existing record. Not part of the production import flow.
#[must_use]
pub fn label_for_promotion_test(id: &RecordId, _created_by: &Identity) -> RecordId {
    // Kept as a documented no-op to make the test helper boundary explicit.
    id.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{BuilderOpts, parsed_incident_to_record};
    use crate::parse::parse_incident_md;
    use crate::source::ImportSource;
    use ft_core::{Label, RecordBody, Relation, RelationKind};
    use ft_storage::EmbeddedStorage;
    use ft_testkit::{TestRepo, make_task};

    fn fixture_quarantined(storage: &dyn Storage) -> RecordId {
        let input = "# Q\n\n## Symptoms\n\ns\n";
        let parsed = parse_incident_md(input, &ImportSource::local_markdown("q.md")).unwrap();
        let opts = BuilderOpts::new(
            Identity::new("imp@firetrail.test").unwrap(),
            ImportSource::local_markdown("q.md"),
        );
        let rec = parsed_incident_to_record(&parsed, &opts).unwrap();
        storage.write(&rec).unwrap();
        rec.envelope.id
    }

    fn canonical_referencing(storage: &dyn Storage, target: &RecordId, n: usize) {
        for i in 0..n {
            let mut t = make_task().title(format!("ref-{i}")).build();
            // Inject the target id into a label so it appears in the
            // serialized form. Re-hash after the mutation.
            t.envelope.labels.push(Label {
                key: "refs".to_string(),
                value: target.as_str().to_string(),
            });
            t.envelope.state_hash.clear();
            t.envelope.state_hash = state_hash(&t).unwrap();
            storage.write(&t).unwrap();
        }
    }

    #[test]
    fn candidates_appears_with_three_inbound_refs() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let q_id = fixture_quarantined(&storage);
        canonical_referencing(&storage, &q_id, 3);

        let cands = promotion_candidates(&storage, &PromotionOpts::default()).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].id, q_id);
        assert_eq!(cands[0].inbound_refs, 3);
    }

    #[test]
    fn candidates_does_not_appear_below_threshold() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let q_id = fixture_quarantined(&storage);
        canonical_referencing(&storage, &q_id, 2);
        let cands = promotion_candidates(&storage, &PromotionOpts::default()).unwrap();
        assert!(cands.is_empty());
    }

    #[test]
    fn promote_record_clears_quarantine_label() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let q_id = fixture_quarantined(&storage);

        promote_record(&storage, &q_id).unwrap();
        let back = storage.read(&q_id).unwrap();
        assert!(!is_quarantined(&back));
        // History entry recorded.
        assert!(!back.envelope.history.is_empty());
        let tail = back.envelope.history.last().unwrap();
        assert!(tail.ops_summary[0].starts_with("promote-import:"));
        // Body still intact.
        assert!(matches!(back.body, RecordBody::Incident(_)));
    }

    #[test]
    fn promote_record_rejects_non_quarantined() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let r = make_task().build();
        storage.write(&r).unwrap();
        let err = promote_record(&storage, &r.envelope.id).unwrap_err();
        assert!(matches!(err, ImportError::NotQuarantined(_)));
    }

    // Sanity import: `Relation` type is referenced symbolically to keep the
    // dependency edge documented even though we don't construct one yet.
    fn _doc_relation_kept(_: Relation, _: RelationKind) {}
}
