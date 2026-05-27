//! Free-form labels and PR-time history entries.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::identity::Identity;

/// Free-form `key=value` label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Label {
    /// Label key.
    pub key: String,
    /// Label value.
    pub value: String,
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
}
