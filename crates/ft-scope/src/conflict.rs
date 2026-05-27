//! Cross-scope decision conflict detection.
//!
//! ADR-0004 calls out that decisions touching shared code can collide between
//! teams. This module surfaces a particular kind of collision: **the same
//! decision title declared in two or more scopes, with different decision
//! bodies**. Same title + same body is treated as benign duplication (likely
//! a mirror or stub); same title + diverging body is a real conflict that
//! needs human resolution.
//!
//! The "external identifier" for a Decision is its [`Decision::title`]
//! (compared case-insensitively after trimming surrounding whitespace). This
//! mirrors how ADRs are typically referenced (`ADR-0004: ...`) and avoids
//! tying detection to the opaque `RecordId` hash, which by construction
//! differs whenever any byte of the body changes.

use std::collections::BTreeMap;

use ft_core::{Decision, Record, RecordBody, RecordId};

/// Two or more decisions with the same external identifier whose bodies
/// disagree across scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictingDecision {
    /// The external identifier (normalised decision title) shared by the
    /// conflicting records.
    pub external_id: String,
    /// One entry per conflicting record, in input order.
    pub occurrences: Vec<DecisionOccurrence>,
}

/// One record's view of a conflicting decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionOccurrence {
    /// Record id of the decision.
    pub record_id: RecordId,
    /// `owning_scope` from the record envelope (`None` if unscoped).
    pub owning_scope: Option<String>,
    /// Raw decision title (pre-normalisation), for display.
    pub title: String,
    /// Decision body text (`Decision::decision`) at the time of comparison.
    pub body: String,
}

/// Detect decisions that share an external identifier across distinct scopes
/// but disagree on the decision body.
///
/// Algorithm:
///
/// 1. Bucket every `Decision` record by normalised title.
/// 2. Within each bucket, keep only buckets that contain at least two
///    distinct `owning_scope` values *and* at least two distinct decision
///    bodies.
/// 3. Emit a [`ConflictingDecision`] per surviving bucket.
///
/// Order of returned conflicts is stable: sorted by normalised external id.
#[must_use]
pub fn detect_conflicting_decisions(records: &[Record]) -> Vec<ConflictingDecision> {
    let mut buckets: BTreeMap<String, Vec<DecisionOccurrence>> = BTreeMap::new();

    for record in records {
        let RecordBody::Decision(decision) = &record.body else {
            continue;
        };
        let key = normalise_title(&decision.title);
        if key.is_empty() {
            continue;
        }
        buckets.entry(key).or_default().push(DecisionOccurrence {
            record_id: record.envelope.id.clone(),
            owning_scope: record.envelope.owning_scope.clone(),
            title: decision.title.clone(),
            body: decision_body(decision),
        });
    }

    let mut out = Vec::new();
    for (external_id, occurrences) in buckets {
        if occurrences.len() < 2 {
            continue;
        }
        // Require multiple distinct scopes — same title inside a single
        // scope is "history within one team", not a cross-scope conflict.
        let mut scopes: Vec<&Option<String>> =
            occurrences.iter().map(|o| &o.owning_scope).collect();
        scopes.sort();
        scopes.dedup();
        if scopes.len() < 2 {
            continue;
        }
        // Require divergence — identical bodies across scopes are benign.
        let mut bodies: Vec<&str> = occurrences.iter().map(|o| o.body.as_str()).collect();
        bodies.sort_unstable();
        bodies.dedup();
        if bodies.len() < 2 {
            continue;
        }
        out.push(ConflictingDecision {
            external_id,
            occurrences,
        });
    }
    out
}

fn normalise_title(raw: &str) -> String {
    raw.trim().to_lowercase()
}

fn decision_body(d: &Decision) -> String {
    d.decision.trim().to_string()
}
