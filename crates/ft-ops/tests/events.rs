//! Event-bus coverage for `ft_ops::tickets`.

use ft_ops::memory::{self, CreateMemoryInput};
use ft_ops::tickets::{
    self, ClaimInput, CloseInput, CreateTaskInput, LinkInput, TicketRelationKind, UnclaimInput,
    UpdateInput,
};
use ft_ops::{Event, EventBus, Identity, Workspace};
use ft_testkit::TestRepo;

fn fixture() -> (TestRepo, Workspace) {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).unwrap();
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .unwrap();
    let ws = Workspace::open(tr.root()).unwrap();
    (tr, ws)
}

fn alice() -> Identity {
    Identity::new("alice@firetrail.test", "Alice")
}

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::too_many_lines)]
async fn full_ticket_lifecycle_emits_expected_events() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::new(64);
    let mut rx = bus.subscribe();

    // create
    let created = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "lifecycle".into(),
            description: None,
            epic: None,
            priority: None,
            owner: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let task_id = created.record.envelope.id.as_str().to_string();

    // update (title only — no status change → only TicketUpdated)
    tickets::update(
        &ws,
        &id,
        UpdateInput {
            id: task_id.clone(),
            title: Some("renamed".into()),
            ..Default::default()
        },
        &bus,
    )
    .unwrap();

    // claim
    tickets::claim(
        &ws,
        &id,
        ClaimInput {
            id: task_id.clone(),
            expires: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    // unclaim
    tickets::unclaim(
        &ws,
        &id,
        UnclaimInput {
            id: task_id.clone(),
            takeover: false,
            reason: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    // close
    tickets::close(
        &ws,
        &id,
        CloseInput {
            id: task_id.clone(),
            force: false,
            reason: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    // Collect every event we've received so far (best-effort, non-blocking).
    let mut events = Vec::new();
    while let Ok(env) = rx.try_recv() {
        events.push(env.event);
    }

    let kinds: Vec<&'static str> = events
        .iter()
        .map(|e| match e {
            Event::TicketCreated { .. } => "created",
            Event::TicketUpdated { .. } => "updated",
            Event::TicketTransitioned { .. } => "transitioned",
            Event::TicketClaimed { .. } => "claimed",
            Event::TicketUnclaimed { .. } => "unclaimed",
            Event::TicketClosed { .. } => "closed",
            Event::TicketLinked { .. } => "linked",
            Event::MemoryWritten { .. } => "memory",
            Event::MemoryCreated { .. } => "memory_created",
            Event::MemorySalvaged { .. } => "memory_salvaged",
            _ => "other",
        })
        .collect();

    // Expected sequence: created, updated (from `update`), claimed,
    // unclaimed, transitioned (Open→Closed), closed.
    assert!(kinds.contains(&"created"), "got: {kinds:?}");
    assert!(kinds.contains(&"updated"), "got: {kinds:?}");
    assert!(kinds.contains(&"claimed"), "got: {kinds:?}");
    assert!(kinds.contains(&"unclaimed"), "got: {kinds:?}");
    assert!(kinds.contains(&"transitioned"), "got: {kinds:?}");
    assert!(kinds.contains(&"closed"), "got: {kinds:?}");

    // The TicketClaimed payload carries the actor.
    let claimed = events
        .iter()
        .find_map(|e| match e {
            Event::TicketClaimed { actor, .. } => Some(actor.clone()),
            _ => None,
        })
        .expect("TicketClaimed present");
    assert_eq!(claimed, "alice@firetrail.test");

    // TicketTransitioned should show open → closed (the only transition we
    // performed; the `update` only touched the title).
    let trans = events
        .iter()
        .find_map(|e| match e {
            Event::TicketTransitioned { from, to, .. } => Some((from.clone(), to.clone())),
            _ => None,
        })
        .expect("TicketTransitioned present");
    assert_eq!(trans.0, "open");
    assert_eq!(trans.1, "closed");
}

#[tokio::test(flavor = "current_thread")]
async fn link_emits_ticket_linked_event() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();

    let a = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "a".into(),
            description: None,
            epic: None,
            priority: None,
            owner: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let b = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "b".into(),
            description: None,
            epic: None,
            priority: None,
            owner: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    tickets::link(
        &ws,
        &id,
        LinkInput {
            from: a.record.envelope.id.as_str().to_string(),
            to: b.record.envelope.id.as_str().to_string(),
            kind: TicketRelationKind::Blocks,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let mut found = false;
    while let Ok(env) = rx.try_recv() {
        if let Event::TicketLinked { relation, .. } = env.event {
            assert_eq!(relation, "blocks");
            found = true;
        }
    }
    assert!(found, "expected a TicketLinked event in the stream");
}

#[tokio::test(flavor = "current_thread")]
async fn create_memory_emits_memory_created_with_request_id() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();

    memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "ev test".into(),
            body: "body".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: Some("req-abc".into()),
        },
        &bus,
    )
    .unwrap();

    let mut got = None;
    while let Ok(env) = rx.try_recv() {
        if let Event::MemoryCreated {
            id: mid,
            record_kind,
        } = env.event
        {
            got = Some((mid, record_kind, env.request_id));
            break;
        }
    }
    let (mid, record_kind, req) = got.expect("expected MemoryCreated");
    assert!(mid.starts_with("MEM-"));
    assert_eq!(record_kind, "mem");
    assert_eq!(req.as_deref(), Some("req-abc"));
}
