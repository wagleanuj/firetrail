//! Offboarding sweep helper.
//!
//! When an identity is offboarded ([`crate::registry::IdentityRegistry::offboard`]),
//! every record that still references them as the current claim holder needs
//! to be released. `ft-cli` runs the actual release; the helper here is the
//! pure function that walks an iterator of records and reports which ones
//! need attention.

use ft_core::{Record, RecordBody, RecordId};

/// Return the ids of records whose active claim is held by `identity`.
///
/// The match is performed against [`ft_core::Identity::as_str`]. Only the
/// body variants that carry a `claim` field (Task, Subtask, Bug) are
/// inspected; the memory-kind bodies have no claim and are skipped.
///
/// `identity` is the canonical identifier the record stores — typically an
/// email address. Callers that have a registry id should expand it to every
/// alias before invoking this helper.
pub fn find_live_claims_for(
    records: impl IntoIterator<Item = Record>,
    identity: &str,
) -> Vec<RecordId> {
    records
        .into_iter()
        .filter_map(|r| {
            let claim = match &r.body {
                RecordBody::Task(t) => t.claim.as_ref(),
                RecordBody::Subtask(s) => s.claim.as_ref(),
                RecordBody::Bug(b) => b.claim.as_ref(),
                RecordBody::Epic(_)
                | RecordBody::Incident(_)
                | RecordBody::Finding(_)
                | RecordBody::Runbook(_)
                | RecordBody::Decision(_)
                | RecordBody::Gotcha(_)
                | RecordBody::Memory(_)
                | RecordBody::Doc(_) => None,
            };
            claim.and_then(|c| {
                if c.claimed_by.as_str() == identity {
                    Some(r.envelope.id)
                } else {
                    None
                }
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use ft_core::{
        Claim, Identity, Origin, Priority, RecordBody, RecordEnvelope, RecordId, RecordKind,
        Status, Task,
    };

    fn make_record(_id_seed: &str, claimed_by: Option<&str>) -> Record {
        let minter = Identity::new("creator@example.com").unwrap();
        let id = RecordId::mint(RecordKind::Task, &minter);
        let envelope = RecordEnvelope {
            id,
            kind: RecordKind::Task,
            title: "t".into(),
            status: Status::Ready,
            priority: Priority::P3,
            owner: None,
            created_by: Identity::new("creator@example.com").unwrap(),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            closed_at: None,
            owning_scope: None,
            affected_scopes: vec![],
            applies_to: vec![],
            state_hash: "0".repeat(64),
            prev_state_hash: None,
            labels: vec![],
            history: vec![],
            origin: Origin::Human,
        };
        let claim = claimed_by.map(|email| Claim {
            claimed_by: Identity::new(email).unwrap(),
            claimed_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            claim_source: "cli".into(),
            claim_expires_at: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap(),
        });
        let body = RecordBody::Task(Task {
            description: "x".into(),
            parent_epic: None,
            acceptance_criteria: vec![],
            evidence: vec![],
            claim,
        });
        Record { envelope, body }
    }

    #[test]
    fn finds_claims_by_target_identity() {
        let r1 = make_record("a", Some("alice@example.com"));
        let r2 = make_record("b", Some("bob@example.com"));
        let r3 = make_record("c", None);
        let r4 = make_record("d", Some("alice@example.com"));
        let target_ids: Vec<_> =
            find_live_claims_for([r1.clone(), r2, r3, r4.clone()], "alice@example.com");
        assert_eq!(target_ids.len(), 2);
        assert!(target_ids.contains(&r1.envelope.id));
        assert!(target_ids.contains(&r4.envelope.id));
    }

    #[test]
    fn returns_empty_when_no_match() {
        let r = make_record("a", Some("alice@example.com"));
        let out = find_live_claims_for([r], "ghost@example.com");
        assert!(out.is_empty());
    }
}
