//! Merge-driver tests: AC union, history union, scalar conflict detection.

use chrono::Utc;
use ft_core::{AcStatus, RecordBody, Status};
use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
use ft_pr::merge::{MergeDriverArgs, merge_driver_cli, merge_records};
use ft_testkit::{make_identity, make_task};

fn draft(summary: &str) -> HistoryDraft {
    HistoryDraft {
        merged_via_pr: None,
        timestamp: Utc::now(),
        primary_actor: make_identity(),
        contributors: Vec::new(),
        ops_summary: vec![summary.to_string()],
        ops_count: 1,
        kind: HistoryEntryKind::Update,
    }
}

#[test]
fn merge_concurrent_ac_additions_preserved() {
    let base = make_task().acceptance_criterion("shared").build();

    let mut ours = base.clone();
    if let RecordBody::Task(b) = &mut ours.body {
        b.acceptance_criteria.push(ft_core::AcceptanceCriterion {
            id: "ac-02".into(),
            text: "added-by-ours".into(),
            status: AcStatus::Unchecked,
            evidence_url: None,
            checked_by: None,
            checked_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            proposed: false,
        });
    }
    ours.envelope.state_hash = String::new();
    ours.envelope.state_hash = ft_core::state_hash(&ours).unwrap();

    let mut theirs = base.clone();
    if let RecordBody::Task(b) = &mut theirs.body {
        b.acceptance_criteria.push(ft_core::AcceptanceCriterion {
            id: "ac-02".into(),
            text: "added-by-theirs".into(),
            status: AcStatus::Unchecked,
            evidence_url: None,
            checked_by: None,
            checked_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            proposed: false,
        });
    }
    theirs.envelope.state_hash = String::new();
    theirs.envelope.state_hash = ft_core::state_hash(&theirs).unwrap();

    let result = merge_records(Some(&base), &ours, &theirs).unwrap();
    if let RecordBody::Task(b) = &result.merged.body {
        let texts: Vec<&str> = b
            .acceptance_criteria
            .iter()
            .map(|a| a.text.as_str())
            .collect();
        assert!(texts.contains(&"shared"));
        assert!(texts.contains(&"added-by-ours"));
        assert!(texts.contains(&"added-by-theirs"));
    } else {
        panic!("unexpected body");
    }
    assert!(result.clean());
}

#[test]
fn merge_ac_checked_wins_over_unchecked() {
    let base = make_task().acceptance_criterion("the-ac").build();

    let mut ours = base.clone();
    if let RecordBody::Task(b) = &mut ours.body {
        b.acceptance_criteria[0].status = AcStatus::Checked;
    }
    ours.envelope.state_hash = String::new();
    ours.envelope.state_hash = ft_core::state_hash(&ours).unwrap();

    let theirs = base.clone();

    let result = merge_records(Some(&base), &ours, &theirs).unwrap();
    if let RecordBody::Task(b) = &result.merged.body {
        assert_eq!(b.acceptance_criteria[0].status, AcStatus::Checked);
    } else {
        panic!("unexpected body");
    }
}

#[test]
fn merge_scalar_conflict_when_both_diverge_to_different_values() {
    let base = make_task().title("base").build();

    let mut ours = base.clone();
    ours.envelope.title = "ours-title".to_string();
    ours.envelope.state_hash = String::new();
    ours.envelope.state_hash = ft_core::state_hash(&ours).unwrap();

    let mut theirs = base.clone();
    theirs.envelope.title = "theirs-title".to_string();
    theirs.envelope.state_hash = String::new();
    theirs.envelope.state_hash = ft_core::state_hash(&theirs).unwrap();

    let result = merge_records(Some(&base), &ours, &theirs).unwrap();
    assert!(!result.clean());
    assert!(result.conflicts.iter().any(|c| c.field == "envelope.title"));
}

#[test]
fn merge_history_union_dedupes_by_to_hash() {
    let mut base = make_task().title("with-history").build();
    append_history(&mut base, draft("create")).unwrap();

    let mut ours = base.clone();
    ours.envelope.status = Status::Ready;
    ours.envelope.state_hash = String::new();
    ours.envelope.state_hash = ft_core::state_hash(&ours).unwrap();
    append_history(&mut ours, draft("ours move")).unwrap();

    let mut theirs = base.clone();
    theirs.envelope.priority = ft_core::Priority::P0;
    theirs.envelope.state_hash = String::new();
    theirs.envelope.state_hash = ft_core::state_hash(&theirs).unwrap();
    append_history(&mut theirs, draft("theirs move")).unwrap();

    let result = merge_records(Some(&base), &ours, &theirs).unwrap();
    // Both side's history entries should survive (different to_hash).
    assert!(result.merged.envelope.history.len() >= 3);
}

#[test]
fn merge_driver_cli_writes_to_ours() {
    let tmp = tempfile::tempdir().unwrap();
    let base_p = tmp.path().join("base.json");
    let ours_p = tmp.path().join("ours.json");
    let theirs_p = tmp.path().join("theirs.json");

    let base = make_task().acceptance_criterion("shared").build();
    std::fs::write(&base_p, serde_json::to_vec_pretty(&base).unwrap()).unwrap();

    let mut ours = base.clone();
    if let RecordBody::Task(b) = &mut ours.body {
        b.acceptance_criteria.push(ft_core::AcceptanceCriterion {
            id: "ac-02".into(),
            text: "ours".into(),
            status: AcStatus::Unchecked,
            evidence_url: None,
            checked_by: None,
            checked_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            proposed: false,
        });
    }
    ours.envelope.state_hash = String::new();
    ours.envelope.state_hash = ft_core::state_hash(&ours).unwrap();
    std::fs::write(&ours_p, serde_json::to_vec_pretty(&ours).unwrap()).unwrap();

    let mut theirs = base.clone();
    if let RecordBody::Task(b) = &mut theirs.body {
        b.acceptance_criteria.push(ft_core::AcceptanceCriterion {
            id: "ac-02".into(),
            text: "theirs".into(),
            status: AcStatus::Unchecked,
            evidence_url: None,
            checked_by: None,
            checked_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            proposed: false,
        });
    }
    theirs.envelope.state_hash = String::new();
    theirs.envelope.state_hash = ft_core::state_hash(&theirs).unwrap();
    std::fs::write(&theirs_p, serde_json::to_vec_pretty(&theirs).unwrap()).unwrap();

    let out = merge_driver_cli(&MergeDriverArgs {
        base_path: base_p,
        ours_path: ours_p.clone(),
        theirs_path: theirs_p,
    })
    .unwrap();
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.conflict_count, 0);

    let merged_bytes = std::fs::read(&ours_p).unwrap();
    let merged: ft_core::Record = serde_json::from_slice(&merged_bytes).unwrap();
    if let RecordBody::Task(b) = &merged.body {
        let texts: Vec<&str> = b
            .acceptance_criteria
            .iter()
            .map(|a| a.text.as_str())
            .collect();
        assert!(texts.contains(&"shared"));
        assert!(texts.contains(&"ours"));
        assert!(texts.contains(&"theirs"));
    }
}
