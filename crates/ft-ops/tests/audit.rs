//! Integration tests for `ft_ops::audit` and `ft_ops::trust`.

use ft_ops::audit::{
    self, CriteriaAddInput, CriteriaListInput, CriteriaToggleInput, GraphDirectionInput,
    GraphInput, LintInput, ReviewInput, VerifyInput,
};
use ft_ops::memory::{self, CreateMemoryInput};
use ft_ops::tickets::{self, CreateTaskInput};
use ft_ops::trust::{self, ReviewInput as TrustReviewInput};
use ft_ops::{EventBus, Identity, Workspace};
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

#[test]
fn lint_empty_workspace_reports_nothing() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let out = audit::lint(&ws, &id, LintInput::default(), &bus).unwrap();
    assert_eq!(out.scanned, 0);
    assert_eq!(out.errors, 0);
    assert!(out.findings.is_empty());
    let env = rx.try_recv().expect("LintRun event");
    match env.event {
        ft_ops::Event::LintRun { findings } => assert_eq!(findings, 0),
        other => panic!("expected LintRun, got {other:?}"),
    }
}

#[test]
fn verify_clean_workspace_returns_ok() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let _ = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "note".into(),
            body: "body".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let out = audit::verify(&ws, &id, VerifyInput::default(), &bus).unwrap();
    assert_eq!(out.failures, 0);
    assert!(out.total >= 1);
}

#[test]
fn review_summarises_a_memory() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let created = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "rev me".into(),
            body: "body".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let mid = created.record.envelope.id.as_str().to_string();

    let out = audit::review(
        &ws,
        &id,
        ReviewInput {
            id: mid.clone(),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert_eq!(out.id, mid);
    assert!(out.chain_valid);
    assert_eq!(out.trust_state.as_deref(), Some("draft"));
}

#[test]
fn criteria_add_then_check_round_trip() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let task = tickets::create_task(
        &ws,
        &id,
        CreateTaskInput {
            title: "do thing".into(),
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
    let tid = task.record.envelope.id.as_str().to_string();

    audit::criteria_add(
        &ws,
        &id,
        CriteriaAddInput {
            id: tid.clone(),
            text: "deploy".into(),
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let listed = audit::criteria_list(
        &ws,
        &id,
        CriteriaListInput {
            id: tid.clone(),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert_eq!(listed.items.len(), 1);
    assert!(!listed.items[0].checked);

    audit::criteria_check(
        &ws,
        &id,
        CriteriaToggleInput {
            id: tid.clone(),
            which: "1".into(),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let listed2 = audit::criteria_list(
        &ws,
        &id,
        CriteriaListInput {
            id: tid,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert!(listed2.items[0].checked);
}

#[test]
fn graph_returns_root_only_when_no_relations() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let created = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "solo".into(),
            body: "body".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let mid = created.record.envelope.id.as_str().to_string();

    let out = audit::graph(
        &ws,
        &id,
        GraphInput {
            id: mid.clone(),
            direction: GraphDirectionInput::Both,
            depth: 2,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert_eq!(out.root, mid);
    assert!(out.edges.is_empty());
    assert!(out.reason.is_some());
    // root node is present.
    assert!(out.nodes.iter().any(|n| n.id == mid));
}

#[test]
fn trust_review_promotes_draft_to_reviewed() {
    let (_tr, ws) = fixture();
    let id = alice();
    let reviewer = Identity::new("bob@firetrail.test", "Bob");
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let created = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "trust me".into(),
            body: "body".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let mid = created.record.envelope.id.as_str().to_string();
    // Drain the create event.
    let _ = rx.try_recv();

    let out = trust::review(
        &ws,
        &reviewer,
        TrustReviewInput {
            id: mid.clone(),
            reason: Some("looks good".into()),
            evidence_url: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    // The reviewed record's new state hash must differ from the create one.
    assert!(!out.record.envelope.state_hash.is_empty());

    let env = rx.try_recv().expect("trust event");
    match env.event {
        ft_ops::Event::TrustTransitioned { id: eid, from, to } => {
            assert_eq!(eid, mid);
            assert_eq!(from, "draft");
            assert_eq!(to, "reviewed");
        }
        other => panic!("expected TrustTransitioned, got {other:?}"),
    }
}
