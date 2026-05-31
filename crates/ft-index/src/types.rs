//! Query inputs and result rows for `ft-index`.

use std::time::Duration;

use chrono::{DateTime, Utc};
use ft_core::{Claim, Identity, Priority, RecordId, RecordKind, RelationKind, Status};

/// A row from the `records` table joined with claim summary counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedRecord {
    /// Canonical record id.
    pub id: RecordId,
    /// Record kind.
    pub kind: RecordKind,
    /// Short title.
    pub title: String,
    /// Workflow status.
    pub status: Status,
    /// Priority class.
    pub priority: Priority,
    /// Current owner.
    pub owner: Option<Identity>,
    /// Original author.
    pub created_by: Identity,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last-update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Close timestamp, if status is `Closed`.
    pub closed_at: Option<DateTime<Utc>>,
    /// Scope that owns the record.
    pub owning_scope: Option<String>,
    /// Active claim, if any.
    pub claim: Option<Claim>,
    /// Count of outgoing `blocked-by` edges.
    pub blocked_by_count: u32,
    /// Count of outgoing `blocks` edges.
    pub blocks_count: u32,
    /// Parent record id (from `child-of` / `parent_epic` / `parent_task`).
    pub parent_id: Option<RecordId>,
    /// Total acceptance criteria attached to this record.
    pub criteria_total: u32,
    /// Acceptance criteria with status `checked`.
    pub criteria_met: u32,
}

/// One edge in a dependency walk result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepEdge {
    /// Source of the edge.
    pub from: RecordId,
    /// Target of the edge.
    pub to: RecordId,
    /// What kind of relation this is.
    pub kind: RelationKind,
    /// Distance (in edges) from the walk root.
    pub depth: u32,
}

/// Summary of a full rebuild.
#[derive(Debug, Clone, Default)]
pub struct RebuildReport {
    /// Number of records inserted into `records`.
    pub records_indexed: u64,
    /// Number of edges inserted into `relations`.
    pub relations_indexed: u64,
    /// Wall-clock time.
    pub elapsed: Duration,
}

/// Summary of an incremental refresh.
#[derive(Debug, Clone, Default)]
pub struct RefreshReport {
    /// Records seen for the first time.
    pub records_added: u64,
    /// Records that already existed and were upserted.
    pub records_updated: u64,
    /// Records removed because their file is gone.
    pub records_removed: u64,
    /// Wall-clock time.
    pub elapsed: Duration,
}

/// Sort order for [`ListQuery`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum OrderBy {
    /// Order by priority ascending (`P0` first), then `updated_at` descending.
    #[default]
    Priority,
    /// Order by `created_at` descending.
    CreatedAt,
    /// Order by `updated_at` descending.
    UpdatedAt,
    /// Order by `title` ascending.
    Title,
}

/// Filter set for [`Index::list`](crate::Index::list).
#[derive(Debug, Default, Clone)]
pub struct ListQuery {
    /// Restrict to these kinds (`None` = any kind).
    pub kinds: Option<Vec<RecordKind>>,
    /// Restrict to these statuses (`None` = any status, subject to
    /// `include_closed` / `include_archived`).
    pub statuses: Option<Vec<Status>>,
    /// Restrict to these owners (`None` = any owner, including unowned).
    pub owners: Option<Vec<Identity>>,
    /// Restrict to these owning scopes.
    pub scopes: Option<Vec<String>>,
    /// Restrict to records carrying every `(key, value)` label listed.
    pub labels: Vec<(String, String)>,
    /// Restrict to children of this parent.
    pub parent: Option<RecordId>,
    /// Restrict to records created at or after this timestamp.
    pub created_since: Option<DateTime<Utc>>,
    /// Restrict to records updated at or after this timestamp.
    pub updated_since: Option<DateTime<Utc>>,
    /// Include `Closed` and `Deferred` records (default: false).
    pub include_closed: bool,
    /// Include `Archived` records (default: false).
    pub include_archived: bool,
    /// Cap result count.
    pub limit: Option<u64>,
    /// Skip this many results from the front.
    pub offset: Option<u64>,
    /// Sort order.
    pub order_by: OrderBy,
}

/// Filter set for [`Index::ready`](crate::Index::ready).
#[derive(Debug, Default, Clone)]
pub struct ReadyQuery {
    /// Restrict to these kinds.
    pub kinds: Option<Vec<RecordKind>>,
    /// Restrict to these owners.
    pub owners: Option<Vec<Identity>>,
    /// Restrict to these owning scopes.
    pub scopes: Option<Vec<String>>,
    /// Include records that currently have an active claim (default: false).
    pub include_claimed: bool,
    /// Cap result count.
    pub limit: Option<u64>,
}

/// Direction of [`Index::dependency_walk`](crate::Index::dependency_walk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkDirection {
    /// Follow `blocked-by` edges toward upstream blockers.
    Upstream,
    /// Follow `blocks` edges toward downstream dependents.
    Downstream,
    /// Walk both directions, deduplicating visits.
    Both,
}
