//! Tests for [`ft_trust::is_stale`] and [`ft_trust::StalePolicy`].

use chrono::{Duration, TimeZone, Utc};
use ft_core::{
    Finding, Identity, Origin, Priority, Record, RecordBody, RecordEnvelope, RecordId, RecordKind,
    RiskClass, Status, TrustState,
};
use ft_trust::{StalePolicy, is_stale};

fn alice() -> Identity {
    Identity::new("alice@example.com").unwrap()
}

fn make_finding(trust: TrustState, risk: Option<RiskClass>, updated_days_ago: i64) -> Record {
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let updated_at = now - Duration::days(updated_days_ago);
    let id = RecordId::mint(RecordKind::Finding, &alice());
    Record {
        envelope: RecordEnvelope {
            id,
            kind: RecordKind::Finding,
            title: "f".into(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: alice(),
            created_at: updated_at,
            updated_at,
            closed_at: None,
            owning_scope: None,
            affected_scopes: Vec::new(),
            applies_to: Vec::new(),
            state_hash: "x".repeat(64),
            prev_state_hash: None,
            labels: Vec::new(),
            history: Vec::new(),
            origin: Origin::Human,
        },
        body: RecordBody::Finding(Finding {
            summary: "f".into(),
            risk_class: risk,
            trust,
            ..Finding::default()
        }),
    }
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()
}

#[test]
fn finding_under_threshold_is_not_stale() {
    let r = make_finding(TrustState::Reviewed, None, 30);
    assert!(!is_stale(&r, now(), &StalePolicy::default()));
}

#[test]
fn finding_over_threshold_is_stale() {
    let r = make_finding(TrustState::Reviewed, None, 200);
    assert!(is_stale(&r, now(), &StalePolicy::default()));
}

#[test]
fn already_stale_stays_stale() {
    let r = make_finding(TrustState::Stale, None, 1);
    assert!(is_stale(&r, now(), &StalePolicy::default()));
}

#[test]
fn terminal_state_is_never_stale() {
    for terminal in [
        TrustState::Archived,
        TrustState::Superseded,
        TrustState::Rejected,
        TrustState::Redacted,
    ] {
        let r = make_finding(terminal, None, 9999);
        assert!(!is_stale(&r, now(), &StalePolicy::default()));
    }
}

#[test]
fn high_stakes_records_age_out_faster() {
    // Finding default = 90 days; high-stakes = 180 days. Since min(90, 180) =
    // 90 is the effective threshold, a 100-day-old security Finding is stale.
    let r = make_finding(TrustState::Verified, Some(RiskClass::Security), 100);
    assert!(is_stale(&r, now(), &StalePolicy::default()));
}

#[test]
fn high_stakes_kind_without_per_kind_threshold_uses_high_stakes_window() {
    // Decisions have no per-kind threshold, but a high-stakes decision still
    // ages out at the high-stakes window (180 days by default).
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let updated = now - Duration::days(200);
    let id = RecordId::mint(RecordKind::Decision, &alice());
    let record = Record {
        envelope: RecordEnvelope {
            id,
            kind: RecordKind::Decision,
            title: "d".into(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: alice(),
            created_at: updated,
            updated_at: updated,
            closed_at: None,
            owning_scope: None,
            affected_scopes: Vec::new(),
            applies_to: Vec::new(),
            state_hash: "x".repeat(64),
            prev_state_hash: None,
            labels: Vec::new(),
            history: Vec::new(),
            origin: Origin::Human,
        },
        body: RecordBody::Decision(ft_core::Decision {
            title: "d".into(),
            decision: "do the thing".into(),
            risk_class: Some(RiskClass::Compliance),
            trust: TrustState::Verified,
            ..ft_core::Decision::default()
        }),
    };
    assert!(is_stale(&record, now, &StalePolicy::default()));
}

#[test]
fn decision_without_risk_is_never_stale_by_age() {
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let updated = now - Duration::days(10_000);
    let id = RecordId::mint(RecordKind::Decision, &alice());
    let record = Record {
        envelope: RecordEnvelope {
            id,
            kind: RecordKind::Decision,
            title: "d".into(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: alice(),
            created_at: updated,
            updated_at: updated,
            closed_at: None,
            owning_scope: None,
            affected_scopes: Vec::new(),
            applies_to: Vec::new(),
            state_hash: "x".repeat(64),
            prev_state_hash: None,
            labels: Vec::new(),
            history: Vec::new(),
            origin: Origin::Human,
        },
        body: RecordBody::Decision(ft_core::Decision {
            title: "d".into(),
            decision: "do the thing".into(),
            risk_class: None,
            trust: TrustState::Verified,
            ..ft_core::Decision::default()
        }),
    };
    assert!(!is_stale(&record, now, &StalePolicy::default()));
}

#[test]
fn custom_threshold_via_with_threshold() {
    let policy = StalePolicy::default().with_threshold(RecordKind::Finding, Some(10));
    let r = make_finding(TrustState::Reviewed, None, 11);
    assert!(is_stale(&r, now(), &policy));
}
