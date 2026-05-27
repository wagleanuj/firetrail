//! Roundtrip + property tests for `ft-core`.

use chrono::{TimeZone, Utc};
use ft_core::{
    Bug, Epic, Identity, Origin, Priority, Record, RecordBody, RecordId, RecordKind, Status,
    Subtask, Task, builder::RecordBuilder, hash::state_hash, validate_record_json,
};
use proptest::prelude::*;

fn alice() -> Identity {
    Identity::new("alice@example.com").unwrap()
}

fn make_task(title: &str, priority: Priority, status: Status) -> Record {
    RecordBuilder::new(RecordKind::Task, title, alice())
        .priority(priority)
        .status(status)
        .build()
        .unwrap()
}

#[test]
fn task_roundtrips() {
    let r = make_task("demo", Priority::P1, Status::Ready);
    let json = serde_json::to_string(&r).unwrap();
    let back: Record = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn all_writable_kinds_roundtrip() {
    let alice = alice();
    let epic = RecordBuilder::new(RecordKind::Epic, "e", alice.clone())
        .epic(Epic {
            description: "the epic".into(),
            child_ids: vec![],
        })
        .build()
        .unwrap();
    let task = RecordBuilder::new(RecordKind::Task, "t", alice.clone())
        .task(Task {
            description: "the task".into(),
            parent_epic: Some(epic.envelope.id.clone()),
            ..Task::default()
        })
        .build()
        .unwrap();
    let subtask = RecordBuilder::new(RecordKind::Subtask, "s", alice.clone())
        .subtask(Subtask {
            description: "child".into(),
            parent_task: task.envelope.id.clone(),
            acceptance_criteria: vec![],
            evidence: vec![],
            claim: None,
        })
        .build()
        .unwrap();
    let bug = RecordBuilder::new(RecordKind::Bug, "b", alice)
        .bug(Bug {
            description: "oops".into(),
            service: Some("api".into()),
            severity: Some("sev3".into()),
            ..Bug::default()
        })
        .build()
        .unwrap();

    for r in [epic, task, subtask, bug] {
        let s = serde_json::to_string(&r).unwrap();
        let back: Record = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}

#[test]
fn memory_kinds_roundtrip_via_serde_even_though_builder_refuses() {
    // Construct directly to bypass the builder.
    let alice = alice();
    let id = RecordId::mint(RecordKind::Finding, &alice);
    let r = Record {
        envelope: ft_core::RecordEnvelope {
            id: id.clone(),
            kind: RecordKind::Finding,
            title: "finding".into(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: alice.clone(),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            closed_at: None,
            owning_scope: None,
            affected_scopes: vec![],
            applies_to: vec![],
            state_hash: String::new(),
            prev_state_hash: None,
            labels: vec![],
            history: vec![],
            origin: Origin::Imported,
        },
        body: RecordBody::Finding(ft_core::Finding::default()),
    };
    let mut r = r;
    r.envelope.state_hash = state_hash(&r).unwrap();
    let s = serde_json::to_string(&r).unwrap();
    let back: Record = serde_json::from_str(&s).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
}

#[test]
fn schema_validates_builder_output_for_all_kinds() {
    let alice = alice();
    let records = [
        RecordBuilder::new(RecordKind::Epic, "e", alice.clone())
            .build()
            .unwrap(),
        RecordBuilder::new(RecordKind::Task, "t", alice.clone())
            .build()
            .unwrap(),
        RecordBuilder::new(RecordKind::Bug, "b", alice.clone())
            .build()
            .unwrap(),
        RecordBuilder::new(RecordKind::Subtask, "s", alice.clone())
            .subtask(Subtask {
                description: "x".into(),
                parent_task: RecordId::mint(RecordKind::Task, &alice),
                acceptance_criteria: vec![],
                evidence: vec![],
                claim: None,
            })
            .build()
            .unwrap(),
    ];
    for r in records {
        let v = serde_json::to_value(&r).unwrap();
        validate_record_json(&v).expect("must validate");
    }
}

// ----- Property tests -----

fn arb_priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        Just(Priority::P0),
        Just(Priority::P1),
        Just(Priority::P2),
        Just(Priority::P3),
        Just(Priority::P4),
    ]
}

fn arb_status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::Open),
        Just(Status::Ready),
        Just(Status::InProgress),
        Just(Status::Review),
        Just(Status::Blocked),
        Just(Status::Closed),
        Just(Status::Deferred),
        Just(Status::Archived),
    ]
}

fn arb_writable_kind() -> impl Strategy<Value = RecordKind> {
    prop_oneof![
        Just(RecordKind::Epic),
        Just(RecordKind::Task),
        Just(RecordKind::Bug),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prop_roundtrip(
        title in "[a-zA-Z0-9]{1,40}",
        priority in arb_priority(),
        status in arb_status(),
        kind in arb_writable_kind(),
    ) {
        let r = RecordBuilder::new(kind, title, alice())
            .priority(priority)
            .status(status)
            .build()
            .unwrap();
        let s = serde_json::to_string(&r).unwrap();
        let back: Record = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(r, back);
    }

    #[test]
    fn prop_state_hash_excludes_hash_fields(
        title in "[a-zA-Z0-9]{1,40}",
        priority in arb_priority(),
        garbage in "[a-f0-9]{64}",
    ) {
        let r = make_task(&title, priority, Status::Open);
        let h1 = state_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.envelope.state_hash = garbage.clone();
        r2.envelope.prev_state_hash = Some(garbage);
        let h2 = state_hash(&r2).unwrap();
        prop_assert_eq!(h1, h2);
    }

    #[test]
    fn prop_canonical_json_is_byte_stable(
        title in "[a-zA-Z0-9]{1,40}",
        priority in arb_priority(),
    ) {
        let r = make_task(&title, priority, Status::Open);
        let a = ft_core::canonical_json(&r).unwrap();
        let b = ft_core::canonical_json(&r).unwrap();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn prop_mint_unique(
        n in 100usize..=500,
    ) {
        let alice = alice();
        let mut set = std::collections::HashSet::new();
        for _ in 0..n {
            let id = RecordId::mint(RecordKind::Task, &alice);
            prop_assert!(set.insert(id));
        }
    }
}

#[test]
fn record_id_short_length_invariant() {
    let id = RecordId::mint(RecordKind::Task, &alice());
    for n in [6usize, 7, 8, 16, 32, 64] {
        let s = id.short(n);
        assert_eq!(s.len(), "TASK-".len() + n);
    }
}
