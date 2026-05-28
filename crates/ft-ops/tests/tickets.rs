//! Integration tests for `ft_ops::tickets`.
//!
//! Each test spins up an isolated `TestRepo` (git + `.firetrail/` skeleton),
//! writes a minimal `config.yml` so [`Workspace::open`] is happy, and then
//! exercises ops directly. We deliberately do NOT shell out to the CLI here —
//! ops should be testable without the binary.

use ft_ops::tickets::{
    self, BoardInput, ClaimInput, CloseInput, CreateBugInput, CreateEpicInput, CreateSubtaskInput,
    CreateTaskInput, LinkInput, ListInput, ShowInput, TicketPriority, TicketRelationKind,
    TicketStatusFilter, UnclaimInput, UpdateInput,
};
use ft_ops::{EventBus, Identity, OpsError, Workspace};
use ft_testkit::TestRepo;

/// Build a workspace fixture: a fresh `TestRepo` with `.firetrail/config.yml`
/// written, ready for `Workspace::open`.
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
fn create_task_round_trip_with_show() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let out = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "demo task".into(),
            description: Some("hello".into()),
            epic: None,
            priority: Some(TicketPriority::P1),
            owner: None,
            scope: None,
            labels: vec!["area=tickets".into()],
            request_id: None,
        },
        &bus,
    )
    .expect("create_task");
    let new_id = out.record.envelope.id.as_str().to_string();
    assert!(
        new_id.starts_with("TASK-"),
        "id should be TASK-prefixed: {new_id}"
    );
    assert_eq!(out.record.envelope.title, "demo task");
    assert_eq!(out.record.envelope.labels.len(), 1);

    let shown = tickets::show(&ws, &id, ShowInput { id: new_id.clone() }, &bus).expect("show");
    assert_eq!(shown.record.envelope.id.as_str(), new_id);
    assert!(shown.relations.is_empty());
}

#[test]
fn create_epic_then_task_with_epic_parent() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let epic = tickets::create_epic(
        &ws,
        &id,
        CreateEpicInput {
            title: "epic".into(),
            description: None,
            priority: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .expect("create_epic");
    let epic_id = epic.record.envelope.id.as_str().to_string();

    let task = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "child".into(),
            description: None,
            epic: Some(epic_id.clone()),
            priority: None,
            owner: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .expect("create_task");
    if let ft_core::RecordBody::Task(t) = &task.record.body {
        assert_eq!(
            t.parent_epic.as_ref().map(|p| p.as_str()),
            Some(epic_id.as_str())
        );
    } else {
        panic!("expected Task body");
    }
}

#[test]
fn subtask_requires_existing_task_parent() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let err = tickets::create_subtask(
        &ws,
        &id,
        CreateSubtaskInput {
            title: "x".into(),
            parent: "TASK-deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            description: None,
            priority: None,
            owner: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected not found");
    assert!(matches!(err, OpsError::NotFound { .. }), "{err:?}");
}

#[test]
fn create_bug_carries_service_and_severity() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let out = tickets::create_bug(
        &ws,
        &id,
        CreateBugInput {
            title: "broken".into(),
            description: None,
            service: Some("auth".into()),
            severity: Some("sev2".into()),
            priority: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .expect("create_bug");
    if let ft_core::RecordBody::Bug(b) = &out.record.body {
        assert_eq!(b.service.as_deref(), Some("auth"));
        assert_eq!(b.severity.as_deref(), Some("sev2"));
    } else {
        panic!("expected Bug body");
    }
}

#[test]
fn update_changes_title_and_priority() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let created = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "old".into(),
            description: None,
            epic: None,
            priority: Some(TicketPriority::P3),
            owner: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .expect("create");
    let task_id = created.record.envelope.id.as_str().to_string();

    let updated = tickets::update(
        &ws,
        &id,
        UpdateInput {
            id: task_id,
            title: Some("new".into()),
            priority: Some(TicketPriority::P0),
            ..Default::default()
        },
        &bus,
    )
    .expect("update");
    assert_eq!(updated.record.envelope.title, "new");
    assert_eq!(updated.record.envelope.priority, ft_core::Priority::P0);
}

#[test]
fn update_with_no_fields_is_rejected() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();
    let created = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "t".into(),
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
    let err = tickets::update(
        &ws,
        &id,
        UpdateInput {
            id: created.record.envelope.id.as_str().to_string(),
            ..Default::default()
        },
        &bus,
    )
    .expect_err("expected validation error");
    assert!(matches!(err, OpsError::Validation { .. }), "{err:?}");
}

#[test]
fn close_blocks_when_ac_incomplete_and_force_requires_reason() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let created = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "t".into(),
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

    // No ACs on this task — should close cleanly.
    let closed = tickets::close(
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
    .expect("close");
    assert_eq!(closed.record.envelope.status, ft_core::Status::Closed);

    // Re-closing fails with Conflict.
    let err = tickets::close(
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
    .expect_err("expected conflict");
    assert!(matches!(err, OpsError::Conflict { .. }), "{err:?}");

    // Force without reason fails Validation.
    let err = tickets::close(
        &ws,
        &id,
        CloseInput {
            id: task_id,
            force: true,
            reason: None,
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected validation error");
    assert!(matches!(err, OpsError::Validation { .. }), "{err:?}");
}

#[test]
fn claim_and_takeover_semantics() {
    let (_tr, ws) = fixture();
    let alice_id = alice();
    let bob_id = Identity::new("bob@firetrail.test", "Bob");
    let bus = bus();

    let created = tickets::create_task(
        &ws,
        &alice_id,
        CreateTaskInput {
            title: "t".into(),
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

    // Alice claims.
    tickets::claim(
        &ws,
        &alice_id,
        ClaimInput {
            id: task_id.clone(),
            expires: Some("12h".into()),
            request_id: None,
        },
        &bus,
    )
    .expect("alice claims");

    // Bob tries to claim the same record → Conflict (live claim).
    let err = tickets::claim(
        &ws,
        &bob_id,
        ClaimInput {
            id: task_id.clone(),
            expires: None,
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected conflict");
    assert!(matches!(err, OpsError::Conflict { .. }), "{err:?}");

    // Bob attempts unclaim without takeover → Conflict.
    let err = tickets::unclaim(
        &ws,
        &bob_id,
        UnclaimInput {
            id: task_id.clone(),
            takeover: false,
            reason: None,
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected conflict");
    assert!(matches!(err, OpsError::Conflict { .. }), "{err:?}");

    // Bob with takeover but no reason → Validation.
    let err = tickets::unclaim(
        &ws,
        &bob_id,
        UnclaimInput {
            id: task_id.clone(),
            takeover: true,
            reason: None,
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected validation");
    assert!(matches!(err, OpsError::Validation { .. }), "{err:?}");

    // Bob with takeover + reason → PermissionDenied (live claim, no admin cap).
    let err = tickets::unclaim(
        &ws,
        &bob_id,
        UnclaimInput {
            id: task_id.clone(),
            takeover: true,
            reason: Some("alice is on vacation".into()),
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected permission denied");
    assert!(matches!(err, OpsError::PermissionDenied { .. }), "{err:?}");

    // Alice can release her own claim cleanly.
    let released = tickets::unclaim(
        &ws,
        &alice_id,
        UnclaimInput {
            id: task_id,
            takeover: false,
            reason: None,
            request_id: None,
        },
        &bus,
    )
    .expect("alice unclaims");
    if let ft_core::RecordBody::Task(t) = &released.record.body {
        assert!(t.claim.is_none());
    } else {
        panic!("expected Task body");
    }
}

#[test]
fn link_persists_and_show_surfaces_relations() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

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
    let a_id = a.record.envelope.id.as_str().to_string();
    let b_id = b.record.envelope.id.as_str().to_string();

    let link = tickets::link(
        &ws,
        &id,
        LinkInput {
            from: a_id.clone(),
            to: b_id.clone(),
            kind: TicketRelationKind::RelatedTo,
            request_id: None,
        },
        &bus,
    )
    .expect("link");
    assert_eq!(link.kind, ft_core::RelationKind::RelatedTo);

    let shown = tickets::show(&ws, &id, ShowInput { id: a_id.clone() }, &bus).unwrap();
    assert_eq!(shown.relations.len(), 1);
    assert_eq!(shown.relations[0].kind, ft_core::RelationKind::RelatedTo);

    // self-link rejected
    let err = tickets::link(
        &ws,
        &id,
        LinkInput {
            from: a_id.clone(),
            to: a_id,
            kind: TicketRelationKind::RelatedTo,
            request_id: None,
        },
        &bus,
    )
    .expect_err("expected validation");
    assert!(matches!(err, OpsError::Validation { .. }), "{err:?}");
}

#[test]
fn list_and_board_reflect_created_tickets() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    for n in 0..3 {
        tickets::create_task(
            &ws,
            &id,
            CreateTaskInput {
                title: format!("task {n}"),
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
    }

    let listed = tickets::list(&ws, &id, ListInput::default(), &bus).unwrap();
    assert_eq!(listed.rows.len(), 3);

    let boarded = tickets::board(&ws, &id, BoardInput::default(), &bus).unwrap();
    assert_eq!(
        boarded.todo.len() + boarded.in_progress.len() + boarded.review.len() + boarded.done.len(),
        3
    );

    // Status filter
    let listed = tickets::list(
        &ws,
        &id,
        ListInput {
            status: Some(TicketStatusFilter::Closed),
            ..Default::default()
        },
        &bus,
    )
    .unwrap();
    assert!(listed.rows.is_empty(), "no closed tickets yet");
}

#[test]
fn strict_identity_blocks_unregistered_actor() {
    let tr = TestRepo::new().unwrap();
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).unwrap();
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: true\n",
    )
    .unwrap();
    // Empty registry → nobody passes.
    std::fs::write(firetrail.join("identities.yaml"), "identities: []\n").unwrap();
    let ws = Workspace::open(tr.root()).unwrap();
    let bus = bus();

    let err = tickets::create_task(
        &ws,
        &alice(),
        CreateTaskInput {
            title: "t".into(),
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
    .expect_err("strict mode rejects unregistered identity");
    assert!(matches!(err, OpsError::PermissionDenied { .. }), "{err:?}");
}
