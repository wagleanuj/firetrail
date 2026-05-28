//! Index-backed ticket listing.

use ft_core::Identity as CoreIdentity;
use ft_index::{IndexedRecord, ListQuery, ReadyQuery};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::TicketCtx;

/// Kind filter for [`ListInput::kind`].
///
/// Mirrors the structural kinds (`epic`/`task`/`subtask`/`bug`); memory kinds
/// belong to the `memory` ops module landing in Wave 2.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketKindFilter {
    /// Epic.
    Epic,
    /// Task.
    Task,
    /// Subtask.
    Subtask,
    /// Bug.
    Bug,
}

impl TicketKindFilter {
    fn to_core(self) -> ft_core::RecordKind {
        match self {
            Self::Epic => ft_core::RecordKind::Epic,
            Self::Task => ft_core::RecordKind::Task,
            Self::Subtask => ft_core::RecordKind::Subtask,
            Self::Bug => ft_core::RecordKind::Bug,
        }
    }
}

/// Status filter for [`ListInput::status`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatusFilter {
    /// Open.
    Open,
    /// Ready.
    Ready,
    /// In progress.
    InProgress,
    /// In review.
    Review,
    /// Blocked.
    Blocked,
    /// Closed.
    Closed,
    /// Deferred.
    Deferred,
    /// Archived.
    Archived,
}

impl TicketStatusFilter {
    fn to_core(self) -> ft_core::Status {
        match self {
            Self::Open => ft_core::Status::Open,
            Self::Ready => ft_core::Status::Ready,
            Self::InProgress => ft_core::Status::InProgress,
            Self::Review => ft_core::Status::Review,
            Self::Blocked => ft_core::Status::Blocked,
            Self::Closed => ft_core::Status::Closed,
            Self::Deferred => ft_core::Status::Deferred,
            Self::Archived => ft_core::Status::Archived,
        }
    }
}

/// Input for [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListInput {
    /// Restrict to a kind.
    #[serde(default)]
    pub kind: Option<TicketKindFilter>,
    /// Restrict to a status (implies `include_closed` + `include_archived` so
    /// the status filter is the only gate).
    #[serde(default)]
    pub status: Option<TicketStatusFilter>,
    /// Restrict to a specific owner identity.
    #[serde(default)]
    pub owner: Option<String>,
    /// Restrict to records under a scope id.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap the number of results.
    #[serde(default)]
    pub limit: Option<u64>,
    /// Skip the first N results.
    #[serde(default)]
    pub offset: Option<u64>,
    /// When `true`, returns only unblocked records (records that have no
    /// open blockers and no active claim). Mirrors the CLI's
    /// `firetrail ready` command. When set, the `status` and `offset`
    /// filters are ignored (the index's ready query has its own gating).
    #[serde(default)]
    pub ready: bool,
}

/// One row of [`ListOutput`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListedTicket {
    /// Full canonical id.
    pub id: String,
    /// Record kind (serialized name, e.g. `"task"`).
    pub kind: String,
    /// Title.
    pub title: String,
    /// Status (serialized form, e.g. `"in_progress"`).
    pub status: String,
    /// Priority (lowercase, e.g. `"p1"`).
    pub priority: String,
    /// Owner identity if set.
    pub owner: Option<String>,
    /// Owning scope if set.
    pub scope: Option<String>,
}

impl From<IndexedRecord> for ListedTicket {
    fn from(r: IndexedRecord) -> Self {
        Self {
            id: r.id.as_str().to_string(),
            kind: serde_value_str(&r.kind),
            title: r.title,
            status: serde_value_str(&r.status),
            priority: serde_value_str(&r.priority),
            owner: r.owner.map(|o| o.as_str().to_string()),
            scope: r.owning_scope,
        }
    }
}

fn serde_value_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Output of [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListOutput {
    /// Result rows in the same order returned by the index.
    pub rows: Vec<ListedTicket>,
}

/// `list` op — index-backed ticket query.
///
/// Read-only; emits no events.
pub fn list(
    ws: &Workspace,
    identity: &Identity,
    input: ListInput,
    _events: &EventBus,
) -> Result<ListOutput, OpsError> {
    let ctx = TicketCtx::open(ws, identity, "list")?;
    if input.ready {
        let mut rq = ReadyQuery::default();
        if let Some(k) = input.kind {
            rq.kinds = Some(vec![k.to_core()]);
        }
        if let Some(o) = input.owner {
            let identity = CoreIdentity::new(o.clone())
                .map_err(|e| OpsError::validation("owner", format!("invalid owner: {e}")))?;
            rq.owners = Some(vec![identity]);
        }
        if let Some(s) = input.scope {
            rq.scopes = Some(vec![s]);
        }
        rq.limit = input.limit;
        let rows = ctx
            .index
            .ready(&rq)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("ready: {e}")))?;
        return Ok(ListOutput {
            rows: rows.into_iter().map(ListedTicket::from).collect(),
        });
    }
    let mut q = ListQuery::default();
    if let Some(k) = input.kind {
        q.kinds = Some(vec![k.to_core()]);
    }
    if let Some(s) = input.status {
        q.statuses = Some(vec![s.to_core()]);
        q.include_closed = true;
        q.include_archived = true;
    }
    if let Some(o) = input.owner {
        let identity = CoreIdentity::new(o.clone())
            .map_err(|e| OpsError::validation("owner", format!("invalid owner: {e}")))?;
        q.owners = Some(vec![identity]);
    }
    if let Some(s) = input.scope {
        q.scopes = Some(vec![s]);
    }
    q.limit = input.limit;
    q.offset = input.offset;

    let rows = ctx
        .index
        .list(&q)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list: {e}")))?;
    Ok(ListOutput {
        rows: rows.into_iter().map(ListedTicket::from).collect(),
    })
}
