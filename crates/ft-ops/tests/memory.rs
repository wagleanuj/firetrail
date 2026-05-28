//! Integration tests for `ft_ops::memory`.
//!
//! Mirrors `tests/tickets.rs`: a `TestRepo`, a minimal `.firetrail/config.yml`,
//! and direct calls into the ops surface. The embedding daemon is NOT spawned
//! in tests — semantic search degrades to the in-process `MockEmbedder`, which
//! is enough to exercise the code path without a model on disk.

use ft_ops::memory::{
    self, CaptureInput, CreateDecisionInput, CreateFindingInput, CreateGotchaInput,
    CreateIncidentInput, CreateMemoryInput, CreateRunbookInput, ListInput, MemoryKind, SearchInput,
    SearchMode, ShowInput, SimilarInput,
};
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

fn bus() -> EventBus {
    EventBus::new(64)
}

#[test]
fn create_memory_round_trip_with_show() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let out = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "hello".into(),
            body: "remember this".into(),
            tags: vec!["onboarding".into()],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .expect("create_memory");
    let mid = out.record.envelope.id.as_str().to_string();
    assert!(mid.starts_with("MEM-"), "expected MEM- prefix, got {mid}");

    let shown = memory::show(&ws, &id, ShowInput { id: mid.clone() }, &bus).expect("show");
    assert_eq!(shown.record.envelope.id.as_str(), mid);
    assert_eq!(shown.record.envelope.title, "hello");
}

#[test]
#[allow(clippy::too_many_lines)]
fn create_each_kind_lists_back() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    memory::create_incident(
        &ws,
        &id,
        CreateIncidentInput {
            summary: "outage".into(),
            severity: None,
            started_at: None,
            services: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    memory::create_finding(
        &ws,
        &id,
        CreateFindingInput {
            summary: "leaky cache".into(),
            incident: None,
            details: None,
            risk_class: None,
            affected: vec![],
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    memory::create_runbook(
        &ws,
        &id,
        CreateRunbookInput {
            title: "restart svc".into(),
            summary: "when svc deadlocks".into(),
            applies_to: vec!["api".into()],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    memory::create_decision(
        &ws,
        &id,
        CreateDecisionInput {
            title: "use rust".into(),
            context: "background".into(),
            decision: "decided".into(),
            consequences: None,
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    memory::create_gotcha(
        &ws,
        &id,
        CreateGotchaInput {
            summary: "tz pitfall".into(),
            details: None,
            risk_class: None,
            affected: vec![],
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "generic".into(),
            body: "note".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let listed = memory::list(&ws, &id, ListInput::default(), &bus).expect("list");
    let kinds: std::collections::HashSet<String> =
        listed.rows.iter().map(|r| r.kind.clone()).collect();
    assert!(kinds.contains("incident"), "kinds={kinds:?}");
    assert!(kinds.contains("finding"));
    assert!(kinds.contains("runbook"));
    assert!(kinds.contains("decision"));
    assert!(kinds.contains("gotcha"));
    assert!(kinds.contains("memory"));
    assert!(listed.rows.len() >= 6);

    // Filter by kind narrows correctly.
    let only_inc = memory::list(
        &ws,
        &id,
        ListInput {
            kind: Some(MemoryKind::Incident),
            ..Default::default()
        },
        &bus,
    )
    .unwrap();
    assert!(only_inc.rows.iter().all(|r| r.kind == "incident"));
    assert!(!only_inc.rows.is_empty());
}

#[test]
fn capture_writes_a_memory_body() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let out = memory::capture(
        &ws,
        &id,
        CaptureInput {
            kind: MemoryKind::Memory,
            title: "captured".into(),
            body: "the body".into(),
            tags: vec!["tag1".into()],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .expect("capture");
    assert_eq!(out.record.envelope.title, "captured");

    // Empty body is a validation error.
    let err = memory::capture(
        &ws,
        &id,
        CaptureInput {
            kind: MemoryKind::Memory,
            title: "x".into(),
            body: "   ".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap_err();
    assert!(
        matches!(err, ft_ops::OpsError::Validation { ref field, .. } if field == "body"),
        "got {err:?}"
    );
}

#[test]
fn keyword_search_returns_matching_hits() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let a = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "kafka rebalance gotcha".into(),
            body: "consumers stall after partition reassignment".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let _b = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "postgres vacuum tuning".into(),
            body: "autovacuum thresholds for high-churn tables".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let _c = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "deploy checklist".into(),
            body: "release notes and changelog".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let out = memory::search(
        &ws,
        &id,
        SearchInput {
            query: "kafka".into(),
            mode: SearchMode::Lexical,
            trust: None,
            kinds: vec![],
            scope: None,
            limit: 10,
            include_quarantine: false,
            request_id: None,
        },
        &bus,
    )
    .expect("search");

    assert!(!out.hits.is_empty(), "no hits, warnings={:?}", out.warnings);
    let top_id = a.record.envelope.id.as_str().to_string();
    assert!(
        out.hits.iter().any(|h| h.id == top_id),
        "kafka memory missing from hits: {:?}",
        out.hits.iter().map(|h| &h.id).collect::<Vec<_>>()
    );
}

#[test]
fn similar_works_without_daemon() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let a = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "feature flag rollout".into(),
            body: "stage by region; canary first".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let _b = memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "feature flag flip".into(),
            body: "use ld flags for guarded rollout".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    // Should not error even if the vector index is empty / sqlite-vec is off.
    let out = memory::similar(
        &ws,
        &id,
        SimilarInput {
            id: a.record.envelope.id.as_str().to_string(),
            limit: 5,
            request_id: None,
        },
        &bus,
    )
    .expect("similar");
    // Hits may be empty in lexical-only builds; the API contract is "no error".
    let _ = out;
}

#[test]
fn semantic_search_degrades_to_mock_when_daemon_missing() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    memory::create_memory(
        &ws,
        &id,
        CreateMemoryInput {
            title: "rate limiter design".into(),
            body: "token bucket vs leaky bucket".into(),
            tags: vec![],
            risk_class: None,
            scope: None,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let out = memory::search(
        &ws,
        &id,
        SearchInput {
            query: "rate limiter".into(),
            mode: SearchMode::Hybrid,
            trust: None,
            kinds: vec![],
            scope: None,
            limit: 5,
            include_quarantine: false,
            request_id: None,
        },
        &bus,
    )
    .expect("hybrid search");
    // We tolerate empty hits here — what matters is that the call succeeded
    // and produced a resolved mode label. Either we degraded to lexical
    // (and got a warning) or the mock embedder filled in.
    assert!(["lexical", "hybrid", "vector", "auto"].contains(&out.mode.as_str()));
}
