//! Integration tests for `ft-prime`.

#![allow(clippy::missing_panics_doc)]

use chrono::{TimeZone, Utc};
use ft_core::{
    Epic, Finding, Identity, Memory, Priority, Record, RecordBuilder, RecordKind, Status, Task,
    TrustState,
};
use ft_index::Index;
use ft_prime::{
    ContextPack, OmittedReason, PrimeOptions, prime_for_query, prime_for_task, render_json,
    render_markdown,
};
use ft_storage::{EmbeddedStorage, Storage};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    storage: EmbeddedStorage,
    index: Index,
    target: Record,
    other_task: Record,
    related_finding: Record,
    epic: Record,
    memory: Record,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();

    // EmbeddedStorage::init expects a git repo root; create one.
    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .status()
        .expect("git init");
    assert!(status.success(), "git init failed");

    let storage = EmbeddedStorage::init(&root).unwrap();

    let alice = Identity::new("alice@firetrail.test").unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap();

    // Epic that owns the target task.
    let mut epic = RecordBuilder::new(RecordKind::Epic, "Search epic", alice.clone())
        .epic(Epic {
            description: "Top-level epic about search and prime.".to_string(),
            child_ids: vec![],
        })
        .owning_scope("svc/search")
        .created_at(now)
        .priority(Priority::P1)
        .build()
        .unwrap();

    let target = RecordBuilder::new(RecordKind::Task, "Implement prime crate", alice.clone())
        .task(Task {
            description: "Implement the ft-prime crate end-to-end.".to_string(),
            parent_epic: Some(epic.envelope.id.clone()),
            ..Default::default()
        })
        .owning_scope("svc/search")
        .priority(Priority::P1)
        .status(Status::Ready)
        .created_at(now)
        .build()
        .unwrap();

    // Update epic's child_ids and re-hash.
    if let ft_core::RecordBody::Epic(e) = &mut epic.body {
        e.child_ids.push(target.envelope.id.clone());
    }
    epic.envelope.state_hash = ft_core::state_hash(&epic).unwrap();

    let other_task = RecordBuilder::new(RecordKind::Task, "Unrelated quota work", alice.clone())
        .task(Task {
            description: "Quota work, different scope.".to_string(),
            ..Default::default()
        })
        .owning_scope("svc/quota")
        .priority(Priority::P3)
        .created_at(now)
        .build()
        .unwrap();

    let related_finding = RecordBuilder::new(
        RecordKind::Finding,
        "Search index race condition",
        alice.clone(),
    )
    .finding(Finding {
        summary: "Race when rebuilding the search index.".to_string(),
        details: "Concurrent writers can corrupt the prime cache.".to_string(),
        trust: TrustState::Verified,
        ..Default::default()
    })
    .owning_scope("svc/search")
    .created_at(now)
    .build()
    .unwrap();

    let memory = RecordBuilder::new(RecordKind::Memory, "Prime token budget lore", alice)
        .memory(Memory {
            title: "Prime token budget lore".to_string(),
            body: "ADR-0019 mandates an omitted manifest.".to_string(),
            trust: TrustState::Reviewed,
            ..Default::default()
        })
        .owning_scope("svc/search")
        .created_at(now)
        .build()
        .unwrap();

    for r in [&epic, &target, &other_task, &related_finding, &memory] {
        storage.write(r).unwrap();
    }

    let mut index = Index::open(&root).unwrap();
    index.rebuild_from(&storage).unwrap();

    Fixture {
        _dir: dir,
        storage,
        index,
        target,
        other_task,
        related_finding,
        epic,
        memory,
    }
}

fn opts(max_tokens: usize) -> PrimeOptions {
    PrimeOptions {
        max_tokens,
        now: Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        ..Default::default()
    }
}

#[test]
fn prime_for_task_includes_target_and_relations() {
    let fx = fixture();
    let pack = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(8000)).unwrap();

    let ids: Vec<&str> = pack.items.iter().map(|i| i.id.as_str()).collect();
    assert!(
        ids.contains(&fx.target.envelope.id.as_str()),
        "target must be present: {ids:?}"
    );
    assert!(
        ids.contains(&fx.epic.envelope.id.as_str()),
        "parent epic must be present (structural relation): {ids:?}"
    );
    // Same-scope finding and memory should also surface.
    assert!(
        ids.contains(&fx.related_finding.envelope.id.as_str()),
        "same-scope finding should be present: {ids:?}"
    );
    assert!(
        ids.contains(&fx.memory.envelope.id.as_str()),
        "same-scope memory should be present: {ids:?}"
    );
    // Different scope should not be present.
    assert!(
        !ids.contains(&fx.other_task.envelope.id.as_str()),
        "out-of-scope task should NOT be present: {ids:?}"
    );

    // First item is the target — required, highest priority.
    assert_eq!(pack.items[0].id.as_str(), fx.target.envelope.id.as_str());
    assert!(pack.total_tokens > 0);
    assert_eq!(pack.budget, 8000);
}

#[test]
fn priority_ordering_prefers_higher_trust() {
    use ft_prime::estimate_tokens;
    // estimate_tokens monotonic sanity (also covered in the unit test).
    assert!(estimate_tokens("aaaaaaaa") >= estimate_tokens("aaaa"));

    let fx = fixture();
    let pack = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(8000)).unwrap();
    // Find finding (verified) and memory (reviewed); finding must rank higher.
    let f_score = item_score(&pack, fx.related_finding.envelope.id.as_str());
    let m_score = item_score(&pack, fx.memory.envelope.id.as_str());
    assert!(
        f_score >= m_score,
        "verified finding ({f_score}) must score >= reviewed memory ({m_score})"
    );
}

fn item_score(pack: &ContextPack, id: &str) -> f32 {
    pack.items
        .iter()
        .find(|i| i.id.as_str() == id)
        .map(|i| i.score)
        .expect("item not found")
}

#[test]
fn budget_enforces_truncation_and_omitted_manifest() {
    let fx = fixture();
    // Tiny budget — should force most items into the omitted manifest.
    let pack = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(20)).unwrap();

    // The target and direct relations are required and always present.
    let target_present = pack.items.iter().any(|i| i.id == fx.target.envelope.id);
    assert!(target_present, "target must always be included");

    // At least one item should have been pushed into omitted with budget reason.
    assert!(
        !pack.omitted.is_empty(),
        "expected some items omitted under a tiny budget"
    );
    assert!(
        pack.omitted
            .iter()
            .any(|e| e.reason == OmittedReason::BudgetExceeded),
        "expected at least one BudgetExceeded omission: {:?}",
        pack.omitted
    );

    // Items with a long body should have a truncation marker when their body
    // alone exceeds 25% of the budget.
    let any_truncated = pack
        .items
        .iter()
        .any(|i| i.body_excerpt.contains("...truncated..."));
    assert!(
        any_truncated,
        "expected at least one item to be truncated under a tiny budget"
    );
}

#[test]
fn prime_for_query_matches_substring() {
    let fx = fixture();
    let pack = prime_for_query(&fx.storage, &fx.index, "race condition", &opts(4000)).unwrap();
    let ids: Vec<&str> = pack.items.iter().map(|i| i.id.as_str()).collect();
    assert!(
        ids.contains(&fx.related_finding.envelope.id.as_str()),
        "query should match the finding's title: {ids:?}"
    );
    assert_eq!(pack.query.as_deref(), Some("race condition"));
}

#[test]
fn empty_query_errors() {
    let fx = fixture();
    let err = prime_for_query(&fx.storage, &fx.index, "   ", &opts(1000)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("empty"), "expected EmptyQuery, got: {msg}");
}

#[test]
fn render_markdown_smoke() {
    let fx = fixture();
    let pack = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(8000)).unwrap();
    let md = render_markdown(&pack);
    assert!(md.starts_with("# Firetrail context pack"));
    assert!(md.contains("## Records"));
    assert!(md.contains(fx.target.envelope.id.as_str()));
}

#[test]
fn render_json_round_trips() {
    let fx = fixture();
    let pack = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(8000)).unwrap();
    let value = render_json(&pack);
    let back: ContextPack = serde_json::from_value(value).unwrap();
    assert_eq!(back.items.len(), pack.items.len());
    assert_eq!(back.total_tokens, pack.total_tokens);
    assert_eq!(back.budget, pack.budget);
}

#[test]
fn deterministic_for_same_inputs() {
    let fx = fixture();
    let a = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(8000)).unwrap();
    let b = prime_for_task(&fx.storage, &fx.index, &fx.target.envelope.id, &opts(8000)).unwrap();
    let a_ids: Vec<&str> = a.items.iter().map(|i| i.id.as_str()).collect();
    let b_ids: Vec<&str> = b.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(a_ids, b_ids, "prime output must be deterministic");
}
