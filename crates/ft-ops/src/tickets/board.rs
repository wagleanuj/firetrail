//! Kanban-style snapshot grouped by status.

use ft_core::{Identity as CoreIdentity, Status};
use ft_index::{IndexedRecord, ListQuery};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::TicketCtx;

/// Input for [`board`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BoardInput {
    /// Filter by scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Filter by owner.
    #[serde(default)]
    pub owner: Option<String>,
}

/// A single card on the board.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardCard {
    /// Full canonical id.
    pub id: String,
    /// Short form (first 8 hex chars after the kind tag) for display.
    pub short_id: String,
    /// Title.
    pub title: String,
    /// Priority (lowercase, e.g. `"p1"`).
    pub priority: String,
    /// Owner identity if set.
    pub owner: Option<String>,
}

/// Output of [`board`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardOutput {
    /// Tickets in `Open` / `Ready` status.
    pub todo: Vec<BoardCard>,
    /// Tickets in `InProgress` / `Blocked` status.
    pub in_progress: Vec<BoardCard>,
    /// Tickets in `Review` status.
    pub review: Vec<BoardCard>,
    /// Tickets in `Closed` status.
    pub done: Vec<BoardCard>,
}

/// `board` op — kanban snapshot derived from the index.
///
/// Read-only; emits no events.
pub fn board(
    ws: &Workspace,
    identity: &Identity,
    input: BoardInput,
    _events: &EventBus,
) -> Result<BoardOutput, OpsError> {
    let ctx = TicketCtx::open(ws, identity, "board")?;

    let mut q = ListQuery {
        include_closed: true,
        include_archived: false,
        ..ListQuery::default()
    };
    if let Some(o) = input.owner {
        let identity = CoreIdentity::new(o.clone())
            .map_err(|e| OpsError::validation("owner", format!("invalid owner: {e}")))?;
        q.owners = Some(vec![identity]);
    }
    if let Some(s) = input.scope {
        q.scopes = Some(vec![s]);
    }
    let rows = ctx
        .index
        .list(&q)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list: {e}")))?;
    Ok(build_board(&rows))
}

fn build_board(rows: &[IndexedRecord]) -> BoardOutput {
    let mut todo = Vec::new();
    let mut in_progress = Vec::new();
    let mut review = Vec::new();
    let mut done = Vec::new();
    for r in rows {
        let card = BoardCard {
            id: r.id.as_str().to_string(),
            short_id: r.id.short(8).to_string(),
            title: r.title.clone(),
            priority: format!("{:?}", r.priority).to_lowercase(),
            owner: r.owner.as_ref().map(|o| o.as_str().to_string()),
        };
        match r.status {
            Status::Open | Status::Ready => todo.push(card),
            Status::InProgress | Status::Blocked => in_progress.push(card),
            Status::Review => review.push(card),
            Status::Closed => done.push(card),
            _ => {}
        }
    }
    for col in [&mut todo, &mut in_progress, &mut review, &mut done] {
        col.sort_by(|a, b| a.id.cmp(&b.id));
    }
    BoardOutput {
        todo,
        in_progress,
        review,
        done,
    }
}
