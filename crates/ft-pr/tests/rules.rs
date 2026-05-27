//! End-to-end rule coverage tests. Each rule has at least one positive
//! (clean) and one negative (violation) case driven through a real
//! [`TestRepo`].

use chrono::{Duration, Utc};
use ft_core::{
    AcStatus, Finding, Memory, Record, RecordBody, RiskClass, Status, TrustState, state_hash,
};
use ft_git::Repo;
use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
use ft_pr::{
    PrValidatorOptions, RuleId, Severity, ValidationCache, validate_pr, validate_pr_cached,
};
use ft_storage::{EmbeddedStorage, Storage};
use ft_testkit::{TestRepo, make_bug, make_identity, make_task};

fn open(tr: &TestRepo) -> (EmbeddedStorage, Repo) {
    let storage = EmbeddedStorage::open(tr.root()).unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    (storage, repo)
}

fn draft(kind: HistoryEntryKind, summary: &str) -> HistoryDraft {
    HistoryDraft {
        merged_via_pr: None,
        timestamp: Utc::now(),
        primary_actor: make_identity(),
        contributors: Vec::new(),
        ops_summary: vec![summary.to_string()],
        ops_count: 1,
        kind,
    }
}

fn opts() -> PrValidatorOptions {
    PrValidatorOptions::default()
}

/// Helper: write `record` to the working tree, stage, commit, returning HEAD.
fn write_commit(tr: &TestRepo, storage: &EmbeddedStorage, record: &Record, msg: &str) -> String {
    storage.write(record).unwrap();
    tr.commit_all(msg).unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    repo.head().unwrap().commit_sha
}

// ---------------------------------------------------------------------------
// mixed_commit
// ---------------------------------------------------------------------------

#[test]
fn mixed_commit_memory_only_is_clean() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut m = make_memory_record("memory-only");
    append_history(&mut m, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &m, "add memory");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::MixedCommit),
        "{:?}",
        report.findings
    );
}

#[test]
fn mixed_commit_code_only_is_clean() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    std::fs::write(tr.root().join("hello.txt"), b"hi\n").unwrap();
    tr.commit_all("add code").unwrap();
    let head = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::MixedCommit)
    );
}

#[test]
fn mixed_commit_memory_plus_code_fires() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut m = make_memory_record("mixed-finding");
    append_history(&mut m, draft(HistoryEntryKind::Create, "born")).unwrap();
    storage.write(&m).unwrap();
    std::fs::write(tr.root().join("src.rs"), b"fn x() {}").unwrap();
    tr.commit_all("mixed").unwrap();
    let head = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::MixedCommit && f.severity == Severity::Error),
        "expected mixed_commit error, got {:?}",
        report.findings
    );
    assert!(!report.is_clean());
}

// ---------------------------------------------------------------------------
// incomplete_acceptance
// ---------------------------------------------------------------------------

#[test]
fn incomplete_acceptance_closed_with_all_checked_is_clean() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let mut t = make_task().acceptance_criterion("first").build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    write_commit(&tr, &storage, &t, "create");
    let base = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    // Mark AC checked + close.
    if let RecordBody::Task(body) = &mut t.body {
        body.acceptance_criteria[0].status = AcStatus::Checked;
    }
    t.envelope.status = Status::Closed;
    t.envelope.closed_at = Some(Utc::now());
    t.envelope.updated_at = Utc::now();
    append_history(&mut t, draft(HistoryEntryKind::Update, "close")).unwrap();
    let head = write_commit(&tr, &storage, &t, "close task");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::IncompleteAcceptance),
        "got: {:?}",
        report.findings
    );
}

#[test]
fn incomplete_acceptance_closed_with_unchecked_fires() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let mut t = make_task().acceptance_criterion("first").build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    write_commit(&tr, &storage, &t, "create");
    let base = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    t.envelope.status = Status::Closed;
    t.envelope.closed_at = Some(Utc::now());
    t.envelope.updated_at = Utc::now();
    append_history(&mut t, draft(HistoryEntryKind::Update, "close")).unwrap();
    let head = write_commit(&tr, &storage, &t, "close task");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::IncompleteAcceptance && f.severity == Severity::Error)
    );
    assert!(!report.is_clean());
}

// ---------------------------------------------------------------------------
// evidence_required
// ---------------------------------------------------------------------------

#[test]
fn evidence_required_high_stakes_to_verified_without_evidence_fires() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);

    // Base: Finding in Draft with high-stakes risk class.
    let mut r = make_finding_record("hs-finding", RiskClass::Security, TrustState::Draft);
    append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
    write_commit(&tr, &storage, &r, "create finding");
    let base = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    // Promote to Verified without an evidence marker.
    if let RecordBody::Finding(b) = &mut r.body {
        b.trust = TrustState::Verified;
    }
    r.envelope.updated_at = Utc::now();
    append_history(&mut r, draft(HistoryEntryKind::Update, "promote")).unwrap();
    let head = write_commit(&tr, &storage, &r, "promote finding");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::EvidenceRequired && f.severity == Severity::Error)
    );
}

#[test]
fn evidence_required_high_stakes_to_verified_with_evidence_is_clean() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);

    let mut r = make_finding_record("hs-finding-2", RiskClass::Security, TrustState::Draft);
    append_history(&mut r, draft(HistoryEntryKind::Create, "born")).unwrap();
    write_commit(&tr, &storage, &r, "create finding");
    let base = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    if let RecordBody::Finding(b) = &mut r.body {
        b.trust = TrustState::Verified;
    }
    r.envelope.updated_at = Utc::now();
    append_history(
        &mut r,
        draft(
            HistoryEntryKind::Update,
            "evidence: https://example.com/postmortem-42",
        ),
    )
    .unwrap();
    let head = write_commit(&tr, &storage, &r, "promote with evidence");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::EvidenceRequired),
        "got: {:?}",
        report.findings
    );
}

// ---------------------------------------------------------------------------
// chain_broken
// ---------------------------------------------------------------------------

#[test]
fn chain_broken_tampered_record_fires() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut t = make_task().title("orig").build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    storage.write(&t).unwrap();
    tr.commit_all("add").unwrap();

    // Tamper: rewrite title in the on-disk JSON without touching state_hash.
    let path = storage.path_for(&t.envelope.id);
    let bytes = std::fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["envelope"]["title"] = serde_json::json!("tampered");
    std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    tr.commit_all("tamper").unwrap();
    let head = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::ChainBroken && f.severity == Severity::Error),
        "got: {:?}",
        report.findings
    );
}

#[test]
fn chain_broken_clean_record_passes() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;
    let mut t = make_task().build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &t, "add task");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::ChainBroken)
    );
}

// ---------------------------------------------------------------------------
// secret_leak
// ---------------------------------------------------------------------------

#[test]
fn secret_leak_aws_access_key_fires() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut m = make_memory_record_with_body("AKIA1234567890ABCDEF leaked here");
    append_history(&mut m, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &m, "leak");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::SecretLeak && f.severity == Severity::Error)
    );
}

#[test]
fn secret_leak_clean_body_is_clean() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut m = make_memory_record_with_body("nothing to see here");
    append_history(&mut m, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &m, "clean");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(report.findings.iter().all(|f| f.rule != RuleId::SecretLeak));
}

// ---------------------------------------------------------------------------
// ac_cap_exceeded
// ---------------------------------------------------------------------------

#[test]
fn ac_cap_exceeded_fires_above_default_cap() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut builder = make_task();
    for i in 0..12 {
        builder = builder.acceptance_criterion(format!("ac {i}"));
    }
    let mut t = builder.build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &t, "many acs");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::AcCapExceeded && f.severity == Severity::Warning)
    );
}

#[test]
fn ac_cap_clean_when_under_cap() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;
    let mut t = make_task()
        .acceptance_criterion("a")
        .acceptance_criterion("b")
        .build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &t, "few acs");
    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::AcCapExceeded)
    );
}

// ---------------------------------------------------------------------------
// draft_expired
// ---------------------------------------------------------------------------

#[test]
fn draft_expired_old_draft_fires() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    let mut m = make_memory_record("old-draft");
    m.envelope.created_at = Utc::now() - Duration::days(30);
    // Re-hash since envelope changed.
    m.envelope.state_hash = String::new();
    m.envelope.state_hash = state_hash(&m).unwrap();
    append_history(&mut m, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &m, "old draft");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::DraftExpired && f.severity == Severity::Warning)
    );
}

#[test]
fn draft_expired_fresh_draft_clean() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;
    let mut m = make_memory_record("fresh");
    append_history(&mut m, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &m, "fresh draft");
    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule != RuleId::DraftExpired)
    );
}

// ---------------------------------------------------------------------------
// deprecated_reference
// ---------------------------------------------------------------------------

#[test]
fn deprecated_reference_warns_when_pointing_at_deprecated() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);

    // Create a deprecated finding first and commit.
    let mut dep = make_finding_record("dep-target", RiskClass::Correctness, TrustState::Deprecated);
    append_history(&mut dep, draft(HistoryEntryKind::Create, "born")).unwrap();
    storage.write(&dep).unwrap();
    tr.commit_all("seed deprecated").unwrap();
    let base = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    // New finding superseded_by the deprecated one — pathological but checks
    // the reference walk.
    let mut new_finding =
        make_finding_record("references-dep", RiskClass::Correctness, TrustState::Draft);
    if let RecordBody::Finding(b) = &mut new_finding.body {
        b.superseded_by = Some(dep.envelope.id.clone());
    }
    new_finding.envelope.updated_at = Utc::now();
    new_finding.envelope.state_hash = String::new();
    new_finding.envelope.state_hash = state_hash(&new_finding).unwrap();
    append_history(&mut new_finding, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &new_finding, "add referencing");

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::DeprecatedReference && f.severity == Severity::Warning)
    );
}

// ---------------------------------------------------------------------------
// pr_link_missing
// ---------------------------------------------------------------------------

#[test]
fn pr_link_missing_warns_when_commit_claims_closure_not_in_diff() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;

    // Create a record but do not transition it to Closed; commit message
    // falsely claims to close it.
    let mut b = make_bug().title("not-closed").build();
    append_history(&mut b, draft(HistoryEntryKind::Create, "born")).unwrap();
    storage.write(&b).unwrap();
    let msg = format!("WIP\n\nCloses {}", b.envelope.id.as_str());
    tr.commit_all(&msg).unwrap();
    let head = Repo::open(tr.root()).unwrap().head().unwrap().commit_sha;

    let report = validate_pr(&storage, &repo, &base, &head, &opts()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == RuleId::PrLinkMissing),
        "got: {:?}",
        report.findings
    );
}

// ---------------------------------------------------------------------------
// caching
// ---------------------------------------------------------------------------

#[test]
fn validation_cache_hits_on_repeat_call() {
    let tr = TestRepo::new().unwrap();
    let (storage, repo) = open(&tr);
    let base = repo.head().unwrap().commit_sha;
    let mut t = make_task().build();
    append_history(&mut t, draft(HistoryEntryKind::Create, "born")).unwrap();
    let head = write_commit(&tr, &storage, &t, "add");

    let mut cache = ValidationCache::new();
    let first = validate_pr_cached(&storage, &repo, &base, &head, &opts(), &mut cache).unwrap();
    assert!(cache.len() == 1);
    let second = validate_pr_cached(&storage, &repo, &base, &head, &opts(), &mut cache).unwrap();
    // Cache still has exactly one entry: a hit, not a re-insertion.
    assert!(cache.len() == 1);
    assert_eq!(first.summary.errors, second.summary.errors);
    assert_eq!(first.summary.warnings, second.summary.warnings);
    assert_eq!(first.findings.len(), second.findings.len());
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn make_memory_record(title: &str) -> Record {
    let id = ft_core::RecordId::mint(ft_core::RecordKind::Memory, &make_identity());
    let now = Utc::now();
    let mut r = Record {
        envelope: ft_core::RecordEnvelope {
            id,
            kind: ft_core::RecordKind::Memory,
            title: title.to_string(),
            status: Status::Open,
            priority: ft_core::Priority::P2,
            owner: None,
            created_by: make_identity(),
            created_at: now,
            updated_at: now,
            closed_at: None,
            owning_scope: None,
            affected_scopes: Vec::new(),
            applies_to: Vec::new(),
            state_hash: String::new(),
            prev_state_hash: None,
            labels: Vec::new(),
            history: Vec::new(),
            origin: ft_core::Origin::Human,
        },
        body: RecordBody::Memory(Memory {
            title: title.to_string(),
            body: String::new(),
            tags: Vec::new(),
            related: Vec::new(),
            risk_class: None,
            trust: TrustState::Draft,
        }),
    };
    r.envelope.state_hash = state_hash(&r).unwrap();
    r
}

fn make_memory_record_with_body(body_text: &str) -> Record {
    let mut r = make_memory_record("with-body");
    if let RecordBody::Memory(b) = &mut r.body {
        b.body = body_text.to_string();
    }
    r.envelope.state_hash = String::new();
    r.envelope.state_hash = state_hash(&r).unwrap();
    r
}

fn make_finding_record(title: &str, risk: RiskClass, trust: TrustState) -> Record {
    let id = ft_core::RecordId::mint(ft_core::RecordKind::Finding, &make_identity());
    let now = Utc::now();
    let mut r = Record {
        envelope: ft_core::RecordEnvelope {
            id,
            kind: ft_core::RecordKind::Finding,
            title: title.to_string(),
            status: Status::Open,
            priority: ft_core::Priority::P2,
            owner: None,
            created_by: make_identity(),
            created_at: now,
            updated_at: now,
            closed_at: None,
            owning_scope: None,
            affected_scopes: Vec::new(),
            applies_to: Vec::new(),
            state_hash: String::new(),
            prev_state_hash: None,
            labels: Vec::new(),
            history: Vec::new(),
            origin: ft_core::Origin::Human,
        },
        body: RecordBody::Finding(Finding {
            summary: title.to_string(),
            details: String::new(),
            incident: None,
            risk_class: Some(risk),
            affected_paths: Vec::new(),
            superseded_by: None,
            trust,
        }),
    };
    r.envelope.state_hash = state_hash(&r).unwrap();
    r
}
