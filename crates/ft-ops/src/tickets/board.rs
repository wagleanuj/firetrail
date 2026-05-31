//! Kanban-style snapshot grouped by status.

use ft_core::{Identity as CoreIdentity, RecordKind, Status};
use ft_index::{IndexedRecord, ListQuery, ReadyQuery};
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
    /// When `true`, only return unblocked records (delegates to the ready
    /// index query). Mirrors the `?ready=true` flag on `/api/tickets`.
    #[serde(default)]
    pub ready: bool,
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

/// The four record kinds that belong on the ticket board.
const TICKET_KINDS: [RecordKind; 4] = [
    RecordKind::Epic,
    RecordKind::Task,
    RecordKind::Subtask,
    RecordKind::Bug,
];

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

    if input.ready {
        let mut rq = ReadyQuery {
            kinds: Some(TICKET_KINDS.to_vec()),
            ..ReadyQuery::default()
        };
        if let Some(o) = input.owner {
            let identity = CoreIdentity::new(o.clone())
                .map_err(|e| OpsError::validation("owner", format!("invalid owner: {e}")))?;
            rq.owners = Some(vec![identity]);
        }
        if let Some(s) = input.scope {
            rq.scopes = Some(vec![s]);
        }
        let rows = ctx
            .index
            .ready(&rq)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("ready: {e}")))?;
        return Ok(build_board(&rows));
    }

    let mut q = ListQuery {
        include_closed: true,
        include_archived: false,
        kinds: Some(TICKET_KINDS.to_vec()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{self, CreateDecisionInput, CreateMemoryInput};
    use crate::tickets::{self, CreateTaskInput};
    use crate::{EventBus, Identity, Workspace};
    use ft_testkit::TestRepo;

    fn fixture() -> (TestRepo, Workspace) {
        let tr = TestRepo::new().expect("test repo");
        let firetrail = tr.firetrail_dir();
        std::fs::create_dir_all(&firetrail).expect("mkdir .firetrail");
        std::fs::write(
            firetrail.join("config.yml"),
            "schema_version: 1\nidentity:\n  strict: false\n",
        )
        .expect("write config.yml");
        let ws = Workspace::open(tr.root()).expect("open workspace");
        (tr, ws)
    }

    fn alice() -> Identity {
        Identity::new("alice@firetrail.test", "Alice")
    }

    fn bus() -> EventBus {
        EventBus::new(64)
    }

    #[test]
    fn board_excludes_memory_and_doc_kinds() {
        let (_tr, ws) = fixture();
        let ident = alice();
        let events = bus();

        // One ticket (Task, Open).
        tickets::create_task(
            &ws,
            &ident,
            CreateTaskInput {
                title: "real task".into(),
                description: None,
                epic: None,
                priority: None,
                owner: None,
                scope: None,
                labels: vec![],
                request_id: None,
            },
            &events,
        )
        .expect("create_task");

        // One Memory record (Open after creation).
        memory::create_memory(
            &ws,
            &ident,
            CreateMemoryInput {
                title: "some memory".into(),
                body: "should not appear on board".into(),
                tags: vec![],
                risk_class: None,
                scope: None,
                request_id: None,
            },
            &events,
        )
        .expect("create_memory");

        // One Decision record (Open after creation).
        memory::create_decision(
            &ws,
            &ident,
            CreateDecisionInput {
                title: "use rust".into(),
                context: "because it is fast".into(),
                decision: "decided".into(),
                consequences: None,
                alternatives: vec![],
                status: None,
                risk_class: None,
                scope: None,
                request_id: None,
            },
            &events,
        )
        .expect("create_decision");

        let out = board(&ws, &ident, BoardInput::default(), &events).unwrap();
        let total = out.todo.len() + out.in_progress.len() + out.review.len() + out.done.len();
        assert_eq!(
            total, 1,
            "only the ticket should appear, not memory/decision"
        );
        assert_eq!(out.todo.len(), 1, "the open task should be in todo");
    }
}
