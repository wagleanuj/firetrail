//! Free-form labels and PR-time history entries.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::enums::TrustState;
use crate::identity::Identity;

/// Free-form `key=value` label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Label {
    /// Label key.
    pub key: String,
    /// Label value.
    pub value: String,
}

/// A structured, machine-readable description of *what kind of change* a
/// [`HistoryEntry`] records, beyond the free-form `ops_summary` lines.
///
/// This payload lets downstream consumers (e.g. `ft-pr`'s `evidence_required`
/// rule) read the semantics of a transition instead of substring-matching the
/// human-readable summary. The enum is intentionally open-ended (room for
/// `Status` / `Assignment` variants later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transition {
    /// A trust-state transition (ADR-0013). `evidence_count` is the number of
    /// evidence items the transition shipped with — the `evidence_required`
    /// rule requires this to be `> 0` for high-stakes promotions to
    /// [`TrustState::Verified`].
    Trust {
        /// State the record was in before the transition.
        from: TrustState,
        /// State the record is in after the transition.
        to: TrustState,
        /// Number of evidence items attached to the transition.
        evidence_count: u32,
    },
}

/// A compacted per-PR history entry (ADR-0003).
///
/// `ft-core` declares the type; population and compaction live in `ft-history`
/// from M2. M1 records ship with an empty `history` vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryEntry {
    /// PR number that merged this batch of changes.
    pub merged_via_pr: Option<u64>,
    /// Merge timestamp.
    pub timestamp: DateTime<Utc>,
    /// Primary actor on the PR.
    pub primary_actor: Identity,
    /// Co-authors / reviewers / additional contributors.
    pub contributors: Vec<Identity>,
    /// One-line operation summaries.
    pub ops_summary: Vec<String>,
    /// Total number of operations in the compacted batch.
    pub ops_count: u32,
    /// Prior `state_hash` at the start of this batch.
    pub from_hash: String,
    /// New `state_hash` at the end of this batch.
    pub to_hash: String,
    /// Structured, machine-readable transition payload (e.g. a trust-state
    /// change). `None` for entries that predate this field or that record no
    /// structured transition.
    ///
    /// CRITICAL: `skip_serializing_if` keeps the hash chain stable — entries
    /// with `transition == None` re-serialize byte-identically to records
    /// written before this field existed, so `verify_chain` still passes
    /// repo-wide. Removing it would serialize `null` and rehash every
    /// persisted entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn entry() -> HistoryEntry {
        HistoryEntry {
            merged_via_pr: Some(7),
            timestamp: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
            primary_actor: Identity::new("alice").unwrap(),
            contributors: Vec::new(),
            ops_summary: vec!["update: retitle".to_string()],
            ops_count: 1,
            from_hash: "aaaa".to_string(),
            to_hash: "bbbb".to_string(),
            transition: None,
        }
    }

    #[test]
    fn transition_none_is_omitted_from_serialized_form() {
        // Hash-chain stability: a None transition must NOT appear in the JSON,
        // so entries written before this field existed re-serialize
        // byte-identically (same canonical JSON ⇒ same SHA-256).
        let json = serde_json::to_value(entry()).unwrap();
        assert!(
            json.get("transition").is_none(),
            "transition: None must be skipped during serialization, got: {json}"
        );
    }

    #[test]
    fn transition_none_round_trips() {
        let e = entry();
        let json = serde_json::to_string(&e).unwrap();
        let back: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
        assert_eq!(back.transition, None);
    }

    #[test]
    fn missing_transition_key_deserializes_to_none() {
        // Legacy entries (no `transition` key at all) must deserialize cleanly.
        let legacy = r#"{
            "merged_via_pr": 7,
            "timestamp": "2023-11-14T22:13:20Z",
            "primary_actor": "alice",
            "contributors": [],
            "ops_summary": ["update: retitle"],
            "ops_count": 1,
            "from_hash": "aaaa",
            "to_hash": "bbbb"
        }"#;
        let parsed: HistoryEntry = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.transition, None);
    }

    #[test]
    fn transition_some_serializes_tagged() {
        let mut e = entry();
        e.transition = Some(Transition::Trust {
            from: TrustState::Draft,
            to: TrustState::Verified,
            evidence_count: 2,
        });
        let json = serde_json::to_value(&e).unwrap();
        let t = json.get("transition").expect("present when Some");
        assert_eq!(
            t.get("kind").and_then(serde_json::Value::as_str),
            Some("trust")
        );
        assert_eq!(
            t.get("from").and_then(serde_json::Value::as_str),
            Some("draft")
        );
        assert_eq!(
            t.get("to").and_then(serde_json::Value::as_str),
            Some("verified")
        );
        assert_eq!(
            t.get("evidence_count").and_then(serde_json::Value::as_u64),
            Some(2)
        );
        // Round-trips.
        let back: HistoryEntry = serde_json::from_value(json).unwrap();
        assert_eq!(e, back);
    }
}
