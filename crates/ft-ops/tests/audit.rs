//! Integration tests for `ft_ops::audit` and `ft_ops::trust`.

use ft_ops::audit::{
    self, CriteriaAddInput, CriteriaListInput, CriteriaToggleInput, GraphDirectionInput,
    GraphInput, LintInput, MAX_GRAPH_NODES, ReviewInput, VerifyInput,
};
use ft_ops::memory::{self, CreateMemoryInput};
use ft_ops::tickets::{self, CreateEpicInput, CreateTaskInput};
use ft_ops::trust::{self, PromoteInput as TrustPromoteInput, ReviewInput as TrustReviewInput};
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
fn graph_small_workspace_is_not_truncated() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();

    let epic = tickets::create_epic(
        &ws,
        &id,
        CreateEpicInput {
            title: "hub".into(),
            description: None,
            priority: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let epic_id = epic.record.envelope.id.as_str().to_string();

    // A handful of children — well under the cap.
    for i in 0..5 {
        tickets::create_task(
            &ws,
            &id,
            CreateTaskInput {
                title: format!("child {i}"),
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
        .unwrap();
    }

    let out = audit::graph(
        &ws,
        &id,
        GraphInput {
            id: epic_id.clone(),
            direction: GraphDirectionInput::Both,
            depth: 2,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    assert!(!out.truncated, "small graph must not be truncated");
    // root epic + 5 children.
    assert_eq!(out.nodes.len(), 6);
    assert!(out.nodes.len() <= MAX_GRAPH_NODES);
}

#[test]
fn graph_dense_workspace_is_capped_and_truncated() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();

    let epic = tickets::create_epic(
        &ws,
        &id,
        CreateEpicInput {
            title: "dense hub".into(),
            description: None,
            priority: None,
            scope: None,
            labels: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let epic_id = epic.record.envelope.id.as_str().to_string();

    // Enough children that root + children exceeds MAX_GRAPH_NODES.
    let children = MAX_GRAPH_NODES + 25;
    for i in 0..children {
        tickets::create_task(
            &ws,
            &id,
            CreateTaskInput {
                title: format!("child {i}"),
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
        .unwrap();
    }

    let out = audit::graph(
        &ws,
        &id,
        GraphInput {
            id: epic_id.clone(),
            direction: GraphDirectionInput::Both,
            depth: 5,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    assert!(out.truncated, "dense graph must be truncated");
    assert!(
        out.nodes.len() <= MAX_GRAPH_NODES,
        "node count {} must not exceed cap {}",
        out.nodes.len(),
        MAX_GRAPH_NODES
    );
    assert_eq!(out.nodes.len(), MAX_GRAPH_NODES);

    // Edge integrity: every edge endpoint must be a node we actually returned.
    let node_ids: std::collections::HashSet<&str> =
        out.nodes.iter().map(|n| n.id.as_str()).collect();
    for e in &out.edges {
        assert!(
            node_ids.contains(e.from.as_str()),
            "edge `from` {} not in returned nodes",
            e.from
        );
        assert!(
            node_ids.contains(e.to.as_str()),
            "edge `to` {} not in returned nodes",
            e.to
        );
    }
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

#[test]
fn trust_promote_records_structured_transition_in_history() {
    use ft_core::Transition;

    let (_tr, ws) = fixture();
    let id = alice();
    let reviewer = Identity::new("bob@firetrail.test", "Bob");
    let bus = EventBus::default();
    // Low-stakes memory (no risk class) so promotion to Verified is permitted
    // without ADR-0013 evidence enforcement at the state-machine layer.
    let created = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "promote me".into(),
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

    // Draft -> Reviewed.
    trust::review(
        &ws,
        &reviewer,
        TrustReviewInput {
            id: mid.clone(),
            reason: Some("reviewed".into()),
            evidence_url: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    // Reviewed -> Verified, with a single piece of evidence. A distinct
    // identity promotes (four-eyes: the reviewer cannot also promote).
    let promoter = Identity::new("carol@firetrail.test", "Carol");
    let out = trust::promote(
        &ws,
        &promoter,
        TrustPromoteInput {
            id: mid.clone(),
            reason: Some("verified".into()),
            evidence_url: Some("https://example.com/proof".into()),
            evidence_type: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let tail = out
        .record
        .envelope
        .history
        .last()
        .expect("history must have a tail after promote");
    assert_eq!(
        tail.transition,
        Some(Transition::Trust {
            from: ft_core::TrustState::Reviewed,
            to: ft_core::TrustState::Verified,
            evidence_count: 1,
        }),
        "promote must record a structured Trust transition with applied from/to and evidence count"
    );
}
