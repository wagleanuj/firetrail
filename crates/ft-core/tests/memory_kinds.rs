//! Round-trip + property tests for the M2 memory-kind record bodies.
//!
//! These exercise the new `Incident`, `Finding`, `Runbook`, `Decision`,
//! `Gotcha`, and `Memory` bodies through:
//!
//! - serde JSON round-trip (lossless)
//! - schema validation via `ft_core::validate_record_json`
//! - `state_hash` recomputation (hash matches what the builder stored)
//! - property-based round-trip on arbitrary inputs
//! - schema rejection of structurally-broken records

use chrono::{Duration, TimeZone, Utc};
use ft_core::{
    Decision, DecisionStatus, Finding, Gotcha, Identity, Incident, Memory, Origin, Priority,
    Record, RecordBody, RecordBuilder, RecordId, RecordKind, RiskClass, Runbook, RunbookStep,
    Severity, Status, TrustState, hash::state_hash, validate_record_json,
};
use proptest::prelude::*;

fn alice() -> Identity {
    Identity::new("alice@example.com").unwrap()
}

fn sample_incident() -> Incident {
    let started = Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();
    Incident {
        summary: "checkout 500s spike".into(),
        severity: Severity::Sev1,
        started_at: started,
        resolved_at: Some(started + Duration::minutes(42)),
        services_affected: vec!["checkout".into(), "payments".into()],
        root_cause: Some("downstream timeout".into()),
        findings: vec![],
        runbooks_invoked: vec![],
        risk_class: Some(RiskClass::Availability),
        trust: TrustState::Reviewed,
    }
}

fn sample_runbook() -> Runbook {
    Runbook {
        title: "Drain a misbehaving checkout node".into(),
        summary: "Remove a single replica from the LB and snapshot logs.".into(),
        steps: vec![
            RunbookStep {
                description: "Drain via load balancer".into(),
                command: Some("kubectl drain checkout-0".into()),
                expected_outcome: "node shows SchedulingDisabled".into(),
            },
            RunbookStep {
                description: "Capture last 5 minutes of stdout".into(),
                command: Some("kubectl logs --since=5m checkout-0 > /tmp/r.log".into()),
                expected_outcome: "log file written and non-empty".into(),
            },
        ],
        applies_to: vec!["checkout".into()],
        risk_class: Some(RiskClass::Availability),
        trust: TrustState::Verified,
    }
}

fn build_record(kind: RecordKind, body: RecordBody) -> Record {
    RecordBuilder::new(kind, "memory kind sample", alice())
        .body(body)
        .origin(Origin::Agent)
        .build()
        .expect("memory-kind record must build")
}

// ----- 1. Round-trip / schema / hash for each kind -----

#[test]
fn incident_roundtrips_and_validates() {
    let r = build_record(
        RecordKind::Incident,
        RecordBody::Incident(sample_incident()),
    );
    let json = serde_json::to_string(&r).unwrap();
    let back: Record = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

#[test]
fn finding_roundtrips_and_validates() {
    let finding = Finding {
        summary: "Redis pool exhaustion before CPU alarms".into(),
        details: "Pool fills before CPU alarms fire; alert on pool occupancy.".into(),
        incident: None,
        risk_class: Some(RiskClass::Availability),
        affected_paths: vec!["services/checkout/redis.rs".into()],
        superseded_by: None,
        trust: TrustState::Draft,
    };
    let r = build_record(RecordKind::Finding, RecordBody::Finding(finding));
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

#[test]
fn runbook_roundtrips_and_validates() {
    let r = build_record(RecordKind::Runbook, RecordBody::Runbook(sample_runbook()));
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

#[test]
fn decision_roundtrips_and_validates() {
    let d = Decision {
        title: "Use ONNX for embeddings".into(),
        context: "Need offline embeddings without LLM dependency.".into(),
        decision: "Adopt bge-small-en-v1.5 via ONNX runtime.".into(),
        consequences: "Adds ort dependency; binary size grows ~30MB.".into(),
        alternatives_considered: vec![
            "Call OpenAI embeddings API".into(),
            "Use sentence-transformers via Python sidecar".into(),
        ],
        status: DecisionStatus::Accepted,
        superseded_by: None,
        risk_class: Some(RiskClass::Correctness),
        trust: TrustState::Verified,
    };
    let r = build_record(RecordKind::Decision, RecordBody::Decision(d));
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

#[test]
fn gotcha_roundtrips_and_validates() {
    let g = Gotcha {
        summary: "sqlite-vec ANN ignores filters with NULL columns".into(),
        details: "Returns 0 rows when any filter column is NULL.".into(),
        affected_paths: vec!["crates/ft-search/src/vector.rs".into()],
        risk_class: Some(RiskClass::Correctness),
        trust: TrustState::Draft,
    };
    let r = build_record(RecordKind::Gotcha, RecordBody::Gotcha(g));
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

#[test]
fn generic_memory_roundtrips_and_validates() {
    let m = Memory {
        title: "Q1 oncall retro themes".into(),
        body: "Recurrent: missing dashboards for checkout latency.".into(),
        tags: vec!["retro".into(), "oncall".into()],
        related: vec![],
        risk_class: None,
        trust: TrustState::Draft,
    };
    let r = build_record(RecordKind::Memory, RecordBody::Memory(m));
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);
    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

// ----- 2. Default trust + missing-fields tolerance on deserialize -----

#[test]
fn defaults_trust_to_draft_when_missing_on_disk() {
    // Simulate a pre-M2 record on disk that omits the `trust` field.
    let r = build_record(RecordKind::Finding, RecordBody::Finding(Finding::default()));
    let mut v = serde_json::to_value(&r).unwrap();
    let body = v.get_mut("body").unwrap().as_object_mut().unwrap();
    body.remove("trust");
    let back: Record = serde_json::from_value(v).expect("must accept missing trust");
    match back.body {
        RecordBody::Finding(f) => assert_eq!(f.trust, TrustState::Draft),
        other => panic!("expected Finding, got {other:?}"),
    }
}

// ----- 3. Builder convenience setters work for every memory kind -----

#[test]
fn builder_setters_compile_and_match_kinds() {
    let alice = alice();

    let inc = RecordBuilder::new(RecordKind::Incident, "i", alice.clone())
        .incident(sample_incident())
        .build()
        .unwrap();
    assert!(matches!(inc.body, RecordBody::Incident(_)));

    let find = RecordBuilder::new(RecordKind::Finding, "f", alice.clone())
        .finding(Finding::default())
        .build()
        .unwrap();
    assert!(matches!(find.body, RecordBody::Finding(_)));

    let run = RecordBuilder::new(RecordKind::Runbook, "rb", alice.clone())
        .runbook(sample_runbook())
        .build()
        .unwrap();
    assert!(matches!(run.body, RecordBody::Runbook(_)));

    let dec = RecordBuilder::new(RecordKind::Decision, "d", alice.clone())
        .decision(Decision::default())
        .build()
        .unwrap();
    assert!(matches!(dec.body, RecordBody::Decision(_)));

    let got = RecordBuilder::new(RecordKind::Gotcha, "g", alice.clone())
        .gotcha(Gotcha::default())
        .build()
        .unwrap();
    assert!(matches!(got.body, RecordBody::Gotcha(_)));

    let mem = RecordBuilder::new(RecordKind::Memory, "m", alice)
        .memory(Memory::default())
        .build()
        .unwrap();
    assert!(matches!(mem.body, RecordBody::Memory(_)));
}

// ----- 4. Kind-mismatched body is rejected -----

#[test]
fn rejects_body_kind_mismatch_for_memory_kinds() {
    let err = RecordBuilder::new(RecordKind::Finding, "x", alice())
        .body(RecordBody::Incident(sample_incident()))
        .build()
        .unwrap_err();
    assert!(matches!(err, ft_core::CoreError::InvalidRecord(_)));
}

// ----- 5. Schema rejects malformed memory records -----

#[test]
fn schema_rejects_finding_missing_summary() {
    // Build then strip the required `summary` field; schema must catch it.
    let r = build_record(RecordKind::Finding, RecordBody::Finding(Finding::default()));
    let mut v = serde_json::to_value(&r).unwrap();
    let body = v.get_mut("body").unwrap().as_object_mut().unwrap();
    body.remove("summary");
    assert!(validate_record_json(&v).is_err());
}

#[test]
fn schema_rejects_unknown_severity() {
    let r = build_record(
        RecordKind::Incident,
        RecordBody::Incident(sample_incident()),
    );
    let mut v = serde_json::to_value(&r).unwrap();
    let body = v.get_mut("body").unwrap().as_object_mut().unwrap();
    body.insert("severity".into(), serde_json::json!("sev9"));
    assert!(validate_record_json(&v).is_err());
}

// ----- 6. Property tests for arbitrary memory bodies -----

fn arb_severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Sev1),
        Just(Severity::Sev2),
        Just(Severity::Sev3),
        Just(Severity::Sev4),
    ]
}

fn arb_trust() -> impl Strategy<Value = TrustState> {
    prop_oneof![
        Just(TrustState::Draft),
        Just(TrustState::Reviewed),
        Just(TrustState::Verified),
        Just(TrustState::Stale),
        Just(TrustState::Deprecated),
        Just(TrustState::Archived),
        Just(TrustState::Superseded),
        Just(TrustState::Rejected),
        Just(TrustState::Redacted),
    ]
}

fn arb_risk() -> impl Strategy<Value = Option<RiskClass>> {
    prop_oneof![
        Just(None),
        Just(Some(RiskClass::Security)),
        Just(Some(RiskClass::Availability)),
        Just(Some(RiskClass::DataLoss)),
        Just(Some(RiskClass::Compliance)),
        Just(Some(RiskClass::Performance)),
        Just(Some(RiskClass::Correctness)),
    ]
}

fn arb_decision_status() -> impl Strategy<Value = DecisionStatus> {
    prop_oneof![
        Just(DecisionStatus::Proposed),
        Just(DecisionStatus::Accepted),
        Just(DecisionStatus::Superseded),
        Just(DecisionStatus::Deprecated),
    ]
}

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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn prop_incident_roundtrip(
        title in "[a-zA-Z0-9][a-zA-Z0-9 ]{0,39}",
        summary in "[a-zA-Z0-9 ]{1,80}",
        severity in arb_severity(),
        trust in arb_trust(),
        risk in arb_risk(),
        services in proptest::collection::vec("[a-z]{1,12}", 0..4),
        priority in arb_priority(),
        status in arb_status(),
    ) {
        let started = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let body = Incident {
            summary,
            severity,
            started_at: started,
            resolved_at: None,
            services_affected: services,
            root_cause: None,
            findings: vec![],
            runbooks_invoked: vec![],
            risk_class: risk,
            trust,
        };
        let r = RecordBuilder::new(RecordKind::Incident, title, alice())
            .priority(priority)
            .status(status)
            .incident(body)
            .build()
            .unwrap();
        let s = serde_json::to_string(&r).unwrap();
        let back: Record = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(&r, &back);
        validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
        prop_assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash.clone());
    }

    #[test]
    fn prop_decision_roundtrip(
        title in "[a-zA-Z0-9][a-zA-Z0-9 ]{0,39}",
        decision in "[a-zA-Z0-9 ]{1,120}",
        status in arb_decision_status(),
        trust in arb_trust(),
        risk in arb_risk(),
        alts in proptest::collection::vec("[a-zA-Z ]{1,30}", 0..3),
    ) {
        let body = Decision {
            title: title.clone(),
            context: "ctx".into(),
            decision,
            consequences: "cons".into(),
            alternatives_considered: alts,
            status,
            superseded_by: None,
            risk_class: risk,
            trust,
        };
        let r = RecordBuilder::new(RecordKind::Decision, title, alice())
            .decision(body)
            .build()
            .unwrap();
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        prop_assert_eq!(&r, &back);
        validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    }

    #[test]
    fn prop_finding_roundtrip(
        title in "[a-zA-Z0-9][a-zA-Z0-9 ]{0,39}",
        summary in "[a-zA-Z0-9 ]{1,80}",
        details in "[a-zA-Z0-9 \n]{0,200}",
        trust in arb_trust(),
        risk in arb_risk(),
        paths in proptest::collection::vec("[a-z/]{1,20}", 0..4),
    ) {
        let body = Finding {
            summary,
            details,
            incident: None,
            risk_class: risk,
            affected_paths: paths,
            superseded_by: None,
            trust,
        };
        let r = RecordBuilder::new(RecordKind::Finding, title, alice())
            .finding(body)
            .build()
            .unwrap();
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        prop_assert_eq!(r, back);
    }

    #[test]
    fn prop_runbook_roundtrip(
        title in "[a-zA-Z0-9][a-zA-Z0-9 ]{0,39}",
        summary in "[a-zA-Z0-9 ]{1,80}",
        n_steps in 0usize..6,
        trust in arb_trust(),
        risk in arb_risk(),
    ) {
        let steps: Vec<RunbookStep> = (0..n_steps)
            .map(|i| RunbookStep {
                description: format!("step {i}"),
                command: if i.is_multiple_of(2) { Some(format!("cmd {i}")) } else { None },
                expected_outcome: format!("ok {i}"),
            })
            .collect();
        let body = Runbook {
            title: title.clone(),
            summary,
            steps,
            applies_to: vec!["svc".into()],
            risk_class: risk,
            trust,
        };
        let r = RecordBuilder::new(RecordKind::Runbook, title, alice())
            .runbook(body)
            .build()
            .unwrap();
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        prop_assert_eq!(r, back);
    }

    #[test]
    fn prop_memory_roundtrip(
        title in "[a-zA-Z0-9][a-zA-Z0-9 ]{0,39}",
        body_md in "[a-zA-Z0-9 \n]{0,200}",
        tags in proptest::collection::vec("[a-z]{1,8}", 0..5),
        trust in arb_trust(),
        risk in arb_risk(),
    ) {
        let body = Memory {
            title: title.clone(),
            body: body_md,
            tags,
            related: vec![],
            risk_class: risk,
            trust,
        };
        let r = RecordBuilder::new(RecordKind::Memory, title, alice())
            .memory(body)
            .build()
            .unwrap();
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        prop_assert_eq!(r, back);
    }

    #[test]
    fn prop_gotcha_roundtrip(
        title in "[a-zA-Z0-9][a-zA-Z0-9 ]{0,39}",
        summary in "[a-zA-Z0-9 ]{1,80}",
        trust in arb_trust(),
        risk in arb_risk(),
        paths in proptest::collection::vec("[a-z/]{1,16}", 0..3),
    ) {
        let body = Gotcha {
            summary,
            details: String::new(),
            affected_paths: paths,
            risk_class: risk,
            trust,
        };
        let r = RecordBuilder::new(RecordKind::Gotcha, title, alice())
            .gotcha(body)
            .build()
            .unwrap();
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        prop_assert_eq!(r, back);
    }
}

// ----- 7. RiskClass::is_high_stakes helper -----

#[test]
fn high_stakes_classification_matches_adr_0013() {
    assert!(RiskClass::Security.is_high_stakes());
    assert!(RiskClass::Availability.is_high_stakes());
    assert!(RiskClass::DataLoss.is_high_stakes());
    assert!(RiskClass::Compliance.is_high_stakes());
    assert!(!RiskClass::Performance.is_high_stakes());
    assert!(!RiskClass::Correctness.is_high_stakes());
}

// ----- 8. Incident chain: finding pointing at incident roundtrips intact -----

#[test]
fn finding_can_reference_incident_id() {
    let inc = build_record(
        RecordKind::Incident,
        RecordBody::Incident(sample_incident()),
    );
    let inc_id: RecordId = inc.envelope.id.clone();

    let finding = Finding {
        summary: "linked to incident".into(),
        details: String::new(),
        incident: Some(inc_id.clone()),
        risk_class: None,
        affected_paths: vec![],
        superseded_by: None,
        trust: TrustState::Draft,
    };
    let r = build_record(RecordKind::Finding, RecordBody::Finding(finding));
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    match back.body {
        RecordBody::Finding(f) => assert_eq!(f.incident, Some(inc_id)),
        other => panic!("expected Finding, got {other:?}"),
    }
}
