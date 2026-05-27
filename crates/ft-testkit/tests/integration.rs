//! ft-testkit integration tests.

use ft_core::{Priority, RelationKind, Status};
use ft_testkit::{
    ScenarioRunner, TestRepo, assert_field, assert_hash_consistent, assert_record_exists,
    assert_relation, make_epic, make_identity, make_task,
};

#[test]
fn testrepo_is_isolated() {
    let a = TestRepo::new().unwrap();
    let b = TestRepo::new().unwrap();
    assert_ne!(a.root(), b.root());
    assert!(a.firetrail_dir().is_dir());
    assert!(b.firetrail_dir().is_dir());
}

#[test]
fn testrepo_git_branch_ops() {
    let repo = TestRepo::new().unwrap();
    let current = repo.current_branch().unwrap();
    assert!(!current.is_empty());
    repo.branch("feat/x").unwrap();
    repo.checkout("feat/x").unwrap();
    assert_eq!(repo.current_branch().unwrap(), "feat/x");
}

#[test]
fn testrepo_commit_all_picks_up_new_files() {
    let repo = TestRepo::new().unwrap();
    std::fs::write(repo.root().join("README.md"), "hello\n").unwrap();
    repo.commit_all("add readme").unwrap();
    let log = repo.run("git", &["log", "--oneline"]).unwrap();
    assert!(log.stdout.contains("add readme"), "log={}", log.stdout);
}

#[test]
fn write_record_and_assert_field_and_hash() {
    let repo = TestRepo::new().unwrap();
    let task = make_task()
        .title("Wire up Redis alert")
        .priority(Priority::P1)
        .status(Status::Ready)
        .owner(make_identity())
        .acceptance_criterion("metric exists")
        .build();

    ft_testkit::assertions::write_record(&repo, &task).unwrap();

    assert_record_exists(&repo, &task.envelope.id);
    assert_field(
        &repo,
        &task.envelope.id,
        "title",
        "Wire up Redis alert".to_string(),
    );
    assert_field(&repo, &task.envelope.id, "priority", Priority::P1);
    assert_field(&repo, &task.envelope.id, "status", Status::Ready);
    assert_hash_consistent(&repo, &task.envelope.id);
}

#[test]
fn assert_relation_reads_relations_json() {
    let repo = TestRepo::new().unwrap();
    let task = make_task().build();
    let epic = make_epic().build();

    let relation = ft_core::Relation {
        from: task.envelope.id.clone(),
        to: epic.envelope.id.clone(),
        kind: RelationKind::ChildOf,
        created_at: chrono::Utc::now(),
        created_by: make_identity(),
    };
    let path = repo.firetrail_dir().join("relations.json");
    std::fs::write(&path, serde_json::to_vec(&vec![relation]).unwrap()).unwrap();

    assert_relation(
        &repo,
        &task.envelope.id,
        &epic.envelope.id,
        RelationKind::ChildOf,
    );
}

#[test]
fn factories_roundtrip_through_serde_json() {
    for record in [
        make_task().title("a").build(),
        make_epic().title("b").build(),
        make_task()
            .title("c")
            .acceptance_criterion("first")
            .acceptance_criterion("second")
            .label("k", "v")
            .build(),
    ] {
        let s = serde_json::to_string(&record).unwrap();
        let back: ft_core::Record = serde_json::from_str(&s).unwrap();
        assert_eq!(back, record);
    }
}

#[test]
fn trivial_scenario_runs_end_to_end() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scenarios")
        .join("trivial.yml");
    let report = ScenarioRunner::run(&path).expect("scenario runs");
    assert_eq!(report.steps_run, 2);
    assert!(
        report.failures.is_empty(),
        "scenario failures: {:#?}",
        report.failures
    );
    assert_eq!(report.steps_passed, 2);
    assert_eq!(report.name, "trivial");
}
