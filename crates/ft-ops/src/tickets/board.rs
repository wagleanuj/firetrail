//! Kanban-style snapshot grouped by status.

use ft_core::{Identity as CoreIdentity, RecordKind, Status};
use ft_index::{IndexedRecord, ListQuery, ReadyQuery};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::TicketCtx;

/// Max hops when walking `parent_id` to find the enclosing epic. The domain
/// chain is Epic → Task → Subtask (depth 2); the larger bound leaves room for
/// future nesting while guaranteeing termination on a cyclic/corrupt chain.
const MAX_EPIC_WALK_DEPTH: usize = 8;

/// Wire string for a record kind. Explicit (not `Debug`) so renames fail to compile.
fn record_kind_str(k: RecordKind) -> &'static str {
    match k {
        RecordKind::Epic => "epic",
        RecordKind::Task => "task",
        RecordKind::Subtask => "subtask",
        RecordKind::Bug => "bug",
        RecordKind::Incident => "incident",
        RecordKind::Finding => "finding",
        RecordKind::Runbook => "runbook",
        RecordKind::Decision => "decision",
        RecordKind::Gotcha => "gotcha",
        RecordKind::Memory => "memory",
        RecordKind::Doc => "doc",
    }
}

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
    /// Record kind, lowercase (`"epic"|"task"|"subtask"|"bug"`). Drives the type pill.
    pub kind: String,
    /// Priority (lowercase, e.g. `"p1"`).
    pub priority: String,
    /// Owner identity if set.
    pub owner: Option<String>,
    /// Canonical id of the enclosing epic, resolved by walking `parent_id`. `None` for orphans/epics.
    pub epic_id: Option<String>,
    /// Total acceptance criteria attached to this record.
    pub criteria_total: u32,
    /// Acceptance criteria with status `checked`.
    pub criteria_met: u32,
    /// Direct children of kind `Subtask`.
    pub subtask_count: u32,
    /// Count of outgoing `blocked-by` edges.
    pub blocked_by_count: u32,
}

/// A lightweight epic summary included in [`BoardOutput`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardEpic {
    /// Full canonical id.
    pub id: String,
    /// Short form (first 8 hex chars after the kind tag) for display.
    pub short_id: String,
    /// Title.
    pub title: String,
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
    /// All epics in the result set, sorted by title. Each epic also appears as
    /// a [`BoardCard`] in its status column.
    pub epics: Vec<BoardEpic>,
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

/// Walk `parent_id` links from `start` until an [`RecordKind::Epic`] ancestor
/// is found. The caller must not pass an Epic record (the call site in
/// `build_board` guards this with an `if r.kind == RecordKind::Epic` check).
fn resolve_epic<'a>(
    start: &'a IndexedRecord,
    by_id: &std::collections::HashMap<&str, &'a IndexedRecord>,
) -> Option<String> {
    let mut cur = start;
    for _ in 0..MAX_EPIC_WALK_DEPTH {
        // bound the walk against cyclic/pathological chains
        if cur.kind == RecordKind::Epic {
            return Some(cur.id.as_str().to_string());
        }
        let parent = cur.parent_id.as_ref()?;
        cur = by_id.get(parent.as_str())?;
    }
    None
}

fn build_board(rows: &[IndexedRecord]) -> BoardOutput {
    use std::collections::HashMap;

    let by_id: HashMap<&str, &IndexedRecord> = rows.iter().map(|r| (r.id.as_str(), r)).collect();

    // Count direct Subtask children per parent.
    let mut subtasks: HashMap<&str, u32> = HashMap::new();
    for r in rows {
        if r.kind == RecordKind::Subtask {
            if let Some(p) = &r.parent_id {
                *subtasks.entry(p.as_str()).or_default() += 1;
            }
        }
    }

    let (mut todo, mut in_progress, mut review, mut done) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut epics: Vec<BoardEpic> = Vec::new();

    for r in rows {
        if r.kind == RecordKind::Epic {
            epics.push(BoardEpic {
                id: r.id.as_str().to_string(),
                short_id: r.id.short(8).to_string(),
                title: r.title.clone(),
            });
        }
        let epic_id = if r.kind == RecordKind::Epic {
            None
        } else {
            resolve_epic(r, &by_id)
        };
        let card = BoardCard {
            id: r.id.as_str().to_string(),
            short_id: r.id.short(8).to_string(),
            title: r.title.clone(),
            kind: record_kind_str(r.kind).to_string(),
            priority: format!("{:?}", r.priority).to_lowercase(),
            owner: r.owner.as_ref().map(|o| o.as_str().to_string()),
            epic_id,
            criteria_total: r.criteria_total,
            criteria_met: r.criteria_met,
            subtask_count: *subtasks.get(r.id.as_str()).unwrap_or(&0),
            blocked_by_count: r.blocked_by_count,
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
    epics.sort_by(|a, b| a.title.cmp(&b.title));
    BoardOutput {
        todo,
        in_progress,
        review,
        done,
        epics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{CriteriaAddInput, CriteriaToggleInput, criteria_add, criteria_check};
    use crate::memory::{self, CreateDecisionInput, CreateMemoryInput};
    use crate::tickets::{self, CreateEpicInput, CreateSubtaskInput, CreateTaskInput};
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

    #[test]
    #[allow(clippy::too_many_lines)]
    fn board_resolves_epic_and_counts() {
        let (_tr, ws) = fixture();
        let ident = alice();
        let events = bus();

        // Create epic E.
        let epic = tickets::create_epic(
            &ws,
            &ident,
            CreateEpicInput {
                title: "my epic".into(),
                description: None,
                priority: None,
                scope: None,
                labels: vec![],
                request_id: None,
            },
            &events,
        )
        .expect("create_epic");
        let e_id = epic.record.envelope.id.as_str().to_string();

        // Create task T with parent epic E, 2 acceptance criteria, 1 checked.
        let task = tickets::create_task(
            &ws,
            &ident,
            CreateTaskInput {
                title: "my task".into(),
                description: None,
                epic: Some(e_id.clone()),
                priority: None,
                owner: None,
                scope: None,
                labels: vec![],
                request_id: None,
            },
            &events,
        )
        .expect("create_task");
        let t_id = task.record.envelope.id.as_str().to_string();

        // Add 2 acceptance criteria to task T.
        criteria_add(
            &ws,
            &ident,
            CriteriaAddInput {
                id: t_id.clone(),
                text: "first criterion".into(),
                request_id: None,
            },
            &events,
        )
        .expect("criteria_add 1");
        criteria_add(
            &ws,
            &ident,
            CriteriaAddInput {
                id: t_id.clone(),
                text: "second criterion".into(),
                request_id: None,
            },
            &events,
        )
        .expect("criteria_add 2");

        // Check 1 criterion (ac-01).
        criteria_check(
            &ws,
            &ident,
            CriteriaToggleInput {
                id: t_id.clone(),
                which: "1".into(),
                request_id: None,
            },
            &events,
        )
        .expect("criteria_check");

        // Create subtask S with parent task T.
        let subtask = tickets::create_subtask(
            &ws,
            &ident,
            CreateSubtaskInput {
                title: "my subtask".into(),
                parent: t_id.clone(),
                description: None,
                priority: None,
                owner: None,
                scope: None,
                labels: vec![],
                request_id: None,
            },
            &events,
        )
        .expect("create_subtask");
        let s_id = subtask.record.envelope.id.as_str().to_string();

        let out = board(&ws, &ident, BoardInput::default(), &events).unwrap();

        // Find task card across all columns.
        let all_cards: Vec<&BoardCard> = out
            .todo
            .iter()
            .chain(out.in_progress.iter())
            .chain(out.review.iter())
            .chain(out.done.iter())
            .collect();

        let t = all_cards.iter().find(|c| c.id == t_id).expect("task card");
        assert_eq!(t.kind, "task");
        assert_eq!(t.epic_id.as_deref(), Some(e_id.as_str()));
        assert_eq!((t.criteria_total, t.criteria_met), (2, 1));
        assert_eq!(t.subtask_count, 1);

        // Subtask should resolve its epic through the task parent.
        let s = all_cards
            .iter()
            .find(|c| c.id == s_id)
            .expect("subtask card");
        assert_eq!(s.epic_id.as_deref(), Some(e_id.as_str()));

        // Epic itself should have no epic_id.
        let e = all_cards.iter().find(|c| c.id == e_id).expect("epic card");
        assert_eq!(e.kind, "epic");
        assert_eq!(e.epic_id, None);

        // The epics list should contain the epic.
        assert_eq!(out.epics.len(), 1);
        assert_eq!(out.epics[0].id, e_id);
    }
}
