//! Property tests for state-machine invariants.

use chrono::{TimeZone, Utc};
use ft_core::{Identity, Origin, TrustState};
use ft_trust::{TrustError, TrustTransition, is_terminal, validate_transition};
use proptest::prelude::*;

fn any_trust_state() -> impl Strategy<Value = TrustState> {
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

fn any_origin() -> impl Strategy<Value = Origin> {
    prop_oneof![
        Just(Origin::Human),
        Just(Origin::Agent),
        Just(Origin::Imported),
    ]
}

fn make_id(name: &str) -> Identity {
    Identity::new(name).unwrap()
}

proptest! {
    /// Terminal states stay terminal: no transition out of a terminal state
    /// is ever legal, regardless of target, reviewer, origin, or evidence.
    #[test]
    fn terminal_states_stay_terminal(
        from in any_trust_state().prop_filter("terminal", |s| is_terminal(*s)),
        to in any_trust_state(),
        origin in any_origin(),
    ) {
        let req = TrustTransition::new(
            from,
            to,
            make_id("bob@x.test"),
            origin,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        );
        let result = validate_transition(from, None, &req, &[], &make_id("alice@x.test"));
        let ok = matches!(result, Err(TrustError::IllegalTransition { .. }));
        prop_assert!(ok);
    }

    /// Agents never reach Verified, no matter the prior reviewer count or
    /// reviewer identity.
    #[test]
    fn agents_never_reach_verified(
        prior_count in 0usize..5,
    ) {
        let prior: Vec<Identity> = (0..prior_count)
            .map(|i| make_id(&format!("reviewer{i}@x.test")))
            .collect();
        let req = TrustTransition::new(
            TrustState::Reviewed,
            TrustState::Verified,
            make_id("agentbot@x.test"),
            Origin::Agent,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        );
        let result = validate_transition(
            TrustState::Reviewed,
            None,
            &req,
            &prior,
            &make_id("alice@x.test"),
        );
        let ok = matches!(result, Err(TrustError::AgentCannotPromote { .. }));
        prop_assert!(ok);
    }

    /// Validation is deterministic: the same inputs always produce the same
    /// outcome.
    #[test]
    fn validation_is_deterministic(
        from in any_trust_state(),
        to in any_trust_state(),
        origin in any_origin(),
    ) {
        let req = TrustTransition::new(
            from,
            to,
            make_id("bob@x.test"),
            origin,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        );
        let a = validate_transition(from, None, &req, &[], &make_id("alice@x.test"));
        let b = validate_transition(from, None, &req, &[], &make_id("alice@x.test"));
        prop_assert_eq!(a, b);
    }
}
