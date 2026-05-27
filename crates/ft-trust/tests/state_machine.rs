//! Unit tests for the trust state machine.
//!
//! Covers every legal edge and the typed errors for every illegal edge plus
//! the ADR-0013 acceptance criteria:
//!
//! - Agents cannot promote to Verified (key acceptance).
//! - High-stakes records require evidence for Verified.
//! - Self-review is rejected.
//! - Duplicate reviewers are rejected.
//! - Terminal states stay terminal (a property test in `props.rs`).

use chrono::{TimeZone, Utc};
use ft_core::{
    Evidence, EvidenceKind, Finding, Identity, Origin, RecordBody, RiskClass, TrustState,
};
use ft_trust::{MemoryBody, TrustError, TrustTransition, apply_transition, validate_transition};

fn alice() -> Identity {
    Identity::new("alice@example.com").unwrap()
}
fn bob() -> Identity {
    Identity::new("bob@example.com").unwrap()
}
fn carol() -> Identity {
    Identity::new("carol@example.com").unwrap()
}

fn t(from: TrustState, to: TrustState, reviewer: Identity, origin: Origin) -> TrustTransition {
    TrustTransition::new(
        from,
        to,
        reviewer,
        origin,
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
    )
}

fn fake_evidence() -> Evidence {
    Evidence {
        id: "ev-01".into(),
        kind: EvidenceKind::TestResult,
        url: "https://ci.example/run/1".into(),
        description: None,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        created_by: bob(),
        commit_sha: None,
        symbol_name: None,
        content_hash: None,
    }
}

// ---------------------------------------------------------------------------
// Legal transitions.
// ---------------------------------------------------------------------------

#[test]
fn draft_to_reviewed_succeeds_with_distinct_reviewer() {
    let req = t(
        TrustState::Draft,
        TrustState::Reviewed,
        bob(),
        Origin::Human,
    );
    validate_transition(TrustState::Draft, None, &req, &[], &alice()).unwrap();
}

#[test]
fn reviewed_to_verified_requires_second_distinct_human_reviewer() {
    let req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        carol(),
        Origin::Human,
    );
    validate_transition(TrustState::Reviewed, None, &req, &[bob()], &alice()).unwrap();
}

#[test]
fn deprecated_with_reason_succeeds_from_any_non_terminal_state() {
    for from in [
        TrustState::Draft,
        TrustState::Reviewed,
        TrustState::Verified,
        TrustState::Stale,
    ] {
        let mut req = t(from, TrustState::Deprecated, bob(), Origin::Human);
        req.reason = Some("no longer accurate".into());
        validate_transition(from, None, &req, &[], &alice()).unwrap();
    }
}

#[test]
fn archived_succeeds_from_any_non_terminal_state() {
    for from in [
        TrustState::Draft,
        TrustState::Reviewed,
        TrustState::Verified,
        TrustState::Stale,
        TrustState::Deprecated,
    ] {
        let req = t(from, TrustState::Archived, bob(), Origin::Human);
        validate_transition(from, None, &req, &[], &alice()).unwrap();
    }
}

#[test]
fn superseded_requires_successor() {
    let mut req = t(
        TrustState::Verified,
        TrustState::Superseded,
        bob(),
        Origin::Human,
    );
    assert_eq!(
        validate_transition(TrustState::Verified, None, &req, &[], &alice()),
        Err(TrustError::MissingSuccessor),
    );
    req.successor = Some(ft_core::RecordId::mint(
        ft_core::RecordKind::Finding,
        &alice(),
    ));
    validate_transition(TrustState::Verified, None, &req, &[], &alice()).unwrap();
}

#[test]
fn rejected_only_from_draft_or_reviewed_with_reason() {
    for from in [TrustState::Draft, TrustState::Reviewed] {
        let mut req = t(from, TrustState::Rejected, bob(), Origin::Human);
        req.reason = Some("incorrect claim".into());
        validate_transition(from, None, &req, &[], &alice()).unwrap();
    }

    // Verified → Rejected is illegal.
    let mut req = t(
        TrustState::Verified,
        TrustState::Rejected,
        bob(),
        Origin::Human,
    );
    req.reason = Some("incorrect".into());
    assert!(matches!(
        validate_transition(TrustState::Verified, None, &req, &[], &alice()),
        Err(TrustError::IllegalTransition { .. })
    ));
}

#[test]
fn redacted_requires_reason_and_is_terminal() {
    let mut req = t(
        TrustState::Verified,
        TrustState::Redacted,
        bob(),
        Origin::Human,
    );
    assert!(matches!(
        validate_transition(TrustState::Verified, None, &req, &[], &alice()),
        Err(TrustError::MissingReason { .. })
    ));
    req.reason = Some("contained secret".into());
    validate_transition(TrustState::Verified, None, &req, &[], &alice()).unwrap();
}

#[test]
fn stale_accepted_from_non_terminal() {
    for from in [
        TrustState::Draft,
        TrustState::Reviewed,
        TrustState::Verified,
        TrustState::Deprecated,
    ] {
        let req = t(from, TrustState::Stale, bob(), Origin::Human);
        validate_transition(from, None, &req, &[], &alice()).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Illegal transitions — specific errors.
// ---------------------------------------------------------------------------

#[test]
fn draft_to_verified_directly_is_illegal() {
    let req = t(
        TrustState::Draft,
        TrustState::Verified,
        bob(),
        Origin::Human,
    );
    assert!(matches!(
        validate_transition(TrustState::Draft, None, &req, &[], &alice()),
        Err(TrustError::IllegalTransition { .. })
    ));
}

#[test]
fn terminal_states_have_no_legal_exit() {
    for from in [
        TrustState::Archived,
        TrustState::Superseded,
        TrustState::Rejected,
        TrustState::Redacted,
    ] {
        let req = t(from, TrustState::Draft, bob(), Origin::Human);
        assert!(matches!(
            validate_transition(from, None, &req, &[], &alice()),
            Err(TrustError::IllegalTransition { .. })
        ));
    }
}

#[test]
fn agent_cannot_promote_to_verified() {
    // ADR-0013 key acceptance: agents may never reach Verified.
    let req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        carol(),
        Origin::Agent,
    );
    assert_eq!(
        validate_transition(TrustState::Reviewed, None, &req, &[bob()], &alice()),
        Err(TrustError::AgentCannotPromote {
            to: TrustState::Verified
        }),
    );
}

#[test]
fn self_review_is_rejected() {
    let req = t(
        TrustState::Draft,
        TrustState::Reviewed,
        alice(),
        Origin::Human,
    );
    assert_eq!(
        validate_transition(TrustState::Draft, None, &req, &[], &alice()),
        Err(TrustError::SelfReview),
    );
}

#[test]
fn duplicate_reviewer_is_rejected() {
    let req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        bob(),
        Origin::Human,
    );
    assert_eq!(
        validate_transition(TrustState::Reviewed, None, &req, &[bob()], &alice()),
        Err(TrustError::DuplicateReviewer { reviewer: bob() }),
    );
}

#[test]
fn high_stakes_verified_requires_evidence() {
    let req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        carol(),
        Origin::Human,
    );
    assert_eq!(
        validate_transition(
            TrustState::Reviewed,
            Some(RiskClass::Security),
            &req,
            &[bob()],
            &alice()
        ),
        Err(TrustError::EvidenceRequired {
            kind: TrustState::Verified
        }),
    );
}

#[test]
fn high_stakes_verified_succeeds_with_evidence() {
    let mut req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        carol(),
        Origin::Human,
    );
    req.evidence.push(fake_evidence());
    validate_transition(
        TrustState::Reviewed,
        Some(RiskClass::Availability),
        &req,
        &[bob()],
        &alice(),
    )
    .unwrap();
}

#[test]
fn low_stakes_verified_does_not_require_evidence() {
    let req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        carol(),
        Origin::Human,
    );
    validate_transition(
        TrustState::Reviewed,
        Some(RiskClass::Performance),
        &req,
        &[bob()],
        &alice(),
    )
    .unwrap();
}

#[test]
fn deprecated_requires_reason() {
    let req = t(
        TrustState::Verified,
        TrustState::Deprecated,
        bob(),
        Origin::Human,
    );
    assert_eq!(
        validate_transition(TrustState::Verified, None, &req, &[], &alice()),
        Err(TrustError::MissingReason {
            kind: TrustState::Deprecated
        }),
    );
}

#[test]
fn from_field_must_match_current_state() {
    // The transition declares from=Draft but the record is in Reviewed.
    let req = t(
        TrustState::Draft,
        TrustState::Reviewed,
        bob(),
        Origin::Human,
    );
    assert!(matches!(
        validate_transition(TrustState::Reviewed, None, &req, &[], &alice()),
        Err(TrustError::IllegalTransition { .. })
    ));
}

#[test]
fn insufficient_reviewers_when_prior_set_is_empty_and_reviewer_is_author() {
    // Edge case: the reviewer is the author, prior_reviewers is empty.
    // The author-check fires first → SelfReview, not InsufficientReviewers.
    let req = t(
        TrustState::Reviewed,
        TrustState::Verified,
        alice(),
        Origin::Human,
    );
    assert_eq!(
        validate_transition(TrustState::Reviewed, None, &req, &[], &alice()),
        Err(TrustError::SelfReview),
    );
}

// ---------------------------------------------------------------------------
// apply_transition — mutation and redaction.
// ---------------------------------------------------------------------------

#[test]
fn apply_transition_mutates_trust_field() {
    let mut finding = Finding {
        summary: "thing".into(),
        ..Finding::default()
    };
    let mut body = RecordBody::Finding(finding.clone());
    let mut view = MemoryBody::from_record_body(&mut body).unwrap();
    let req = t(
        TrustState::Draft,
        TrustState::Reviewed,
        bob(),
        Origin::Human,
    );
    apply_transition(&mut view, &req).unwrap();
    assert_eq!(view.trust(), TrustState::Reviewed);
    finding.trust = TrustState::Reviewed;
    // Sanity: applying didn't wipe the summary on non-Redacted transitions.
    if let RecordBody::Finding(f) = &body {
        assert_eq!(f.summary, "thing");
        assert_eq!(f.trust, TrustState::Reviewed);
    }
}

#[test]
fn apply_transition_wipes_body_on_redaction() {
    let finding = Finding {
        summary: "secret".into(),
        details: "very secret".into(),
        affected_paths: vec!["/etc/secrets".into()],
        trust: TrustState::Verified,
        ..Finding::default()
    };
    let mut body = RecordBody::Finding(finding);
    let mut view = MemoryBody::from_record_body(&mut body).unwrap();
    let mut req = t(
        TrustState::Verified,
        TrustState::Redacted,
        bob(),
        Origin::Human,
    );
    req.reason = Some("contained secret".into());
    apply_transition(&mut view, &req).unwrap();
    if let RecordBody::Finding(f) = &body {
        assert!(f.summary.is_empty());
        assert!(f.details.is_empty());
        assert!(f.affected_paths.is_empty());
        assert_eq!(f.trust, TrustState::Redacted);
    } else {
        panic!("body shape changed");
    }
}

#[test]
fn apply_transition_populates_epoch_timestamps() {
    let mut body = RecordBody::Finding(Finding::default());
    let mut view = MemoryBody::from_record_body(&mut body).unwrap();
    let mut req = t(
        TrustState::Draft,
        TrustState::Reviewed,
        bob(),
        Origin::Human,
    );
    req.occurred_at = chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap();
    let applied = apply_transition(&mut view, &req).unwrap();
    assert_ne!(
        applied.occurred_at,
        chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap()
    );
}

#[test]
fn memory_body_rejects_non_memory_records() {
    let mut body = RecordBody::Task(ft_core::Task::default());
    assert!(MemoryBody::from_record_body(&mut body).is_err());
}
