//! Epic roll-up op — read-only snapshot with child counts and a `ready_to_close` flag.

use std::collections::{BTreeMap, HashMap};

use ft_core::{RecordKind, Status};
use ft_index::{IndexedRecord, ListQuery};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::board::{BoardCard, card_from, resolve_epic};
use super::ctx::TicketCtx;

/// The four record kinds relevant to the ticket board / epics roll-up.
const TICKET_KINDS: [RecordKind; 4] = [
    RecordKind::Epic,
    RecordKind::Task,
    RecordKind::Subtask,
    RecordKind::Bug,
];

/// Input for [`epics`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EpicsInput {
    /// Optional scope filter (reserved for future use).
    #[serde(default)]
    pub scope: Option<String>,
}

/// A summary row for one epic in [`EpicsOutput`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpicSummary {
    /// Full canonical id.
    pub id: String,
    /// Short form (first 8 hex chars after the kind tag) for display.
    pub short_id: String,
    /// Title.
    pub title: String,
    /// Status (lowercase, e.g. `"open"`, `"closed"`, `"in_progress"`).
    pub status: String,
    /// Priority (lowercase, e.g. `"p1"`).
    pub priority: String,
    /// Total direct-child tickets (tasks, subtasks, bugs) under this epic.
    pub child_total: u32,
    /// Children whose status is `Closed`.
    pub child_closed: u32,
    /// Acceptance criteria on the epic record itself.
    pub criteria_total: u32,
    /// Acceptance criteria on the epic that are checked.
    pub criteria_met: u32,
    /// `true` when all children are closed AND all own criteria are met AND the
    /// epic is not already closed.
    pub ready_to_close: bool,
}

/// Output of [`epics`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpicsOutput {
    /// One summary per epic, sorted by title.
    pub epics: Vec<EpicSummary>,
    /// Maps epic canonical id → the [`BoardCard`]s for its children.
    pub children: BTreeMap<String, Vec<BoardCard>>,
}

/// Produce a lowercase status string from a [`Status`] value.
fn status_str(s: Status) -> String {
    match s {
        Status::Open => "open",
        Status::Ready => "ready",
        Status::InProgress => "in_progress",
        Status::Blocked => "blocked",
        Status::Review => "review",
        Status::Closed => "closed",
        Status::Deferred => "deferred",
        Status::Archived => "archived",
    }
    .to_string()
}

/// `epics` op — roll-up snapshot with child counts and a `ready_to_close` flag.
///
/// Read-only; emits no events.
pub fn epics(
    ws: &Workspace,
    identity: &Identity,
    input: EpicsInput,
    _events: &EventBus,
) -> Result<EpicsOutput, OpsError> {
    let ctx = TicketCtx::open(ws, identity, "epics")?;

    let mut q = ListQuery {
        include_closed: true,
        include_archived: false,
        kinds: Some(TICKET_KINDS.to_vec()),
        ..ListQuery::default()
    };
    if let Some(s) = input.scope {
        q.scopes = Some(vec![s]);
    }
    let rows = ctx
        .index
        .list(&q)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list: {e}")))?;

    Ok(build_epics_output(&rows))
}

fn build_epics_output(rows: &[IndexedRecord]) -> EpicsOutput {
    // Build a lookup map for the resolve_epic walk.
    let by_id: HashMap<&str, &IndexedRecord> = rows.iter().map(|r| (r.id.as_str(), r)).collect();

    // Count direct Subtask children per parent (needed by card_from).
    let mut subtasks: HashMap<&str, u32> = HashMap::new();
    for r in rows {
        if r.kind == RecordKind::Subtask {
            if let Some(p) = &r.parent_id {
                *subtasks.entry(p.as_str()).or_default() += 1;
            }
        }
    }

    // Collect all epic records.
    let epic_records: Vec<&IndexedRecord> =
        rows.iter().filter(|r| r.kind == RecordKind::Epic).collect();

    // For each non-epic record, resolve to its enclosing epic and accumulate
    // child counts and card lists.
    let mut child_total: HashMap<String, u32> = HashMap::new();
    let mut child_closed: HashMap<String, u32> = HashMap::new();
    let mut children_cards: BTreeMap<String, Vec<BoardCard>> = BTreeMap::new();

    for r in rows {
        if r.kind == RecordKind::Epic {
            continue;
        }
        let Some(epic_id) = resolve_epic(r, &by_id) else {
            continue;
        };
        *child_total.entry(epic_id.clone()).or_default() += 1;
        if r.status == Status::Closed {
            *child_closed.entry(epic_id.clone()).or_default() += 1;
        }
        let subtask_count = *subtasks.get(r.id.as_str()).unwrap_or(&0);
        let card = card_from(r, Some(epic_id.clone()), subtask_count);
        children_cards
            .entry(epic_id.clone())
            .or_default()
            .push(card);
    }

    // Build EpicSummary per epic.
    let mut epics: Vec<EpicSummary> = epic_records
        .iter()
        .map(|e| {
            let eid = e.id.as_str();
            let total = *child_total.get(eid).unwrap_or(&0);
            let closed = *child_closed.get(eid).unwrap_or(&0);
            let crit_total = e.criteria_total;
            let crit_met = e.criteria_met;
            let ready = total > 0
                && closed == total
                && crit_met == crit_total
                && e.status != Status::Closed;
            EpicSummary {
                id: eid.to_string(),
                short_id: e.id.short(8).to_string(),
                title: e.title.clone(),
                status: status_str(e.status),
                priority: format!("{:?}", e.priority).to_lowercase(),
                child_total: total,
                child_closed: closed,
                criteria_total: crit_total,
                criteria_met: crit_met,
                ready_to_close: ready,
            }
        })
        .collect();

    epics.sort_by(|a, b| a.title.cmp(&b.title));

    // Sort child card lists for determinism.
    for cards in children_cards.values_mut() {
        cards.sort_by(|a, b| a.id.cmp(&b.id));
    }

    EpicsOutput {
        epics,
        children: children_cards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tickets::{self, CloseInput, CreateEpicInput, CreateSubtaskInput, CreateTaskInput};
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

    /// Close a ticket by id, ignoring any reason.
    fn close_ticket(ws: &Workspace, id: &str) {
        let ident = alice();
        let events = bus();
        tickets::close(
            ws,
            &ident,
            CloseInput {
                id: id.to_string(),
                reason: None,
                force: false,
                request_id: None,
            },
            &events,
        )
        .expect("close ticket");
    }

    #[test]
    fn epics_flags_ready_to_close() {
        let (_tr, ws) = fixture();
        let ident = alice();
        let events = bus();

        // Epic E with 0 own criteria.
        let epic = tickets::create_epic(
            &ws,
            &ident,
            CreateEpicInput {
                title: "epic alpha".into(),
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

        // Two child tasks under the epic.
        let t1 = tickets::create_task(
            &ws,
            &ident,
            CreateTaskInput {
                title: "task one".into(),
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
        .expect("create_task 1");
        let t1_id = t1.record.envelope.id.as_str().to_string();

        let t2 = tickets::create_task(
            &ws,
            &ident,
            CreateTaskInput {
                title: "task two".into(),
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
        .expect("create_task 2");
        let t2_id = t2.record.envelope.id.as_str().to_string();

        // Close both children.
        close_ticket(&ws, &t1_id);
        close_ticket(&ws, &t2_id);

        let out = epics(&ws, &ident, EpicsInput::default(), &events).unwrap();
        let e = out
            .epics
            .iter()
            .find(|s| s.id == e_id)
            .expect("epic summary");

        assert!(e.ready_to_close, "should be ready to close");
        assert_eq!((e.child_total, e.child_closed), (2, 2));

        // children map should contain both task cards for this epic.
        let child_cards = out.children.get(&e_id).expect("children entry");
        assert_eq!(child_cards.len(), 2);
    }

    #[test]
    fn epics_not_ready_when_child_open() {
        let (_tr, ws) = fixture();
        let ident = alice();
        let events = bus();

        // Epic E.
        let epic = tickets::create_epic(
            &ws,
            &ident,
            CreateEpicInput {
                title: "epic beta".into(),
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

        // One child task — left Open.
        let t = tickets::create_task(
            &ws,
            &ident,
            CreateTaskInput {
                title: "open task".into(),
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
        let t_id = t.record.envelope.id.as_str().to_string();

        let out = epics(&ws, &ident, EpicsInput::default(), &events).unwrap();
        let e = out
            .epics
            .iter()
            .find(|s| s.id == e_id)
            .expect("epic summary");

        assert!(
            !e.ready_to_close,
            "should NOT be ready to close — child is open"
        );
        assert_eq!((e.child_total, e.child_closed), (1, 0));

        // children map should contain the one task card.
        let child_cards = out.children.get(&e_id).expect("children entry");
        assert_eq!(child_cards.len(), 1);
        assert_eq!(child_cards[0].id, t_id);
    }

    #[test]
    fn epics_not_ready_when_no_children() {
        let (_tr, ws) = fixture();
        let ident = alice();
        let events = bus();

        // Epic with no children: child_total = 0 → ready_to_close = false.
        let epic = tickets::create_epic(
            &ws,
            &ident,
            CreateEpicInput {
                title: "epic gamma".into(),
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

        let out = epics(&ws, &ident, EpicsInput::default(), &events).unwrap();
        let e = out
            .epics
            .iter()
            .find(|s| s.id == e_id)
            .expect("epic summary");

        assert!(!e.ready_to_close, "no children → not ready");
        assert_eq!((e.child_total, e.child_closed), (0, 0));
    }

    #[test]
    fn epics_subtask_resolves_to_grandparent_epic() {
        let (_tr, ws) = fixture();
        let ident = alice();
        let events = bus();

        // Epic → Task → Subtask: subtask should count toward the epic's child total.
        let epic = tickets::create_epic(
            &ws,
            &ident,
            CreateEpicInput {
                title: "epic delta".into(),
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

        let task = tickets::create_task(
            &ws,
            &ident,
            CreateTaskInput {
                title: "parent task".into(),
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

        tickets::create_subtask(
            &ws,
            &ident,
            CreateSubtaskInput {
                title: "child subtask".into(),
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

        let out = epics(&ws, &ident, EpicsInput::default(), &events).unwrap();
        let e = out
            .epics
            .iter()
            .find(|s| s.id == e_id)
            .expect("epic summary");

        // task + subtask both resolve to this epic
        assert_eq!(e.child_total, 2, "task + subtask both count");

        // children map should have entries for this epic
        let child_cards = out.children.get(&e_id).expect("children entry");
        assert_eq!(child_cards.len(), 2);
    }
}
