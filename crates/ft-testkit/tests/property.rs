//! Property tests for record factories.
//!
//! Verifies that arbitrary combinations of overrides still produce records
//! that (a) round-trip through `serde_json` and (b) have a `state_hash` that
//! recomputes consistently — the proxy for "ft-core accepts this record".

use ft_core::{Priority, Status};
use ft_testkit::{make_bug, make_epic, make_subtask, make_task};
use proptest::prelude::*;

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

fn arb_nonempty_title() -> impl Strategy<Value = String> {
    // ASCII printable, length 1..32, no leading/trailing whitespace.
    "[a-zA-Z0-9][a-zA-Z0-9 ._-]{0,30}[a-zA-Z0-9]".prop_map(String::from)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

    #[test]
    fn task_factory_always_valid(
        title in arb_nonempty_title(),
        priority in arb_priority(),
        status in arb_status(),
        n_acs in 0usize..4,
    ) {
        let mut b = ft_testkit::make_task().title(title).priority(priority).status(status);
        for i in 0..n_acs {
            b = b.acceptance_criterion(format!("ac text {i}"));
        }
        let r = b.build();
        let s = serde_json::to_string(&r).unwrap();
        let back: ft_core::Record = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(back, r.clone());
        let rehash = ft_core::state_hash(&r).unwrap();
        prop_assert_eq!(rehash, r.envelope.state_hash);
    }

    #[test]
    fn epic_factory_always_valid(
        title in arb_nonempty_title(),
        priority in arb_priority(),
    ) {
        let r = make_epic().title(title).priority(priority).build();
        let s = serde_json::to_string(&r).unwrap();
        let back: ft_core::Record = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(back, r);
    }

    #[test]
    fn bug_factory_always_valid(
        title in arb_nonempty_title(),
        priority in arb_priority(),
    ) {
        let r = make_bug().title(title).priority(priority).build();
        let s = serde_json::to_string(&r).unwrap();
        let back: ft_core::Record = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(back, r);
    }

    #[test]
    fn subtask_factory_always_valid(
        title in arb_nonempty_title(),
    ) {
        let parent = make_task().build();
        let r = make_subtask(parent.envelope.id.clone()).title(title).build();
        let s = serde_json::to_string(&r).unwrap();
        let back: ft_core::Record = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(back, r);
    }
}
