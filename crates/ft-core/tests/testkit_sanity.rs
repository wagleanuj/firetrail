//! Cross-crate sanity check: records produced by ft-testkit factories
//! validate cleanly through ft-core's schema validator and `state_hash`
//! recomputation.
//!
//! Per ft-testkit's spec acceptance #7, ft-testkit is consumed by ft-core's
//! integration tests as a sanity check. This intentionally creates a
//! dev-dependency cycle (ft-testkit depends on ft-core); Cargo handles
//! dev-dep cycles fine because they only matter when building ft-core's
//! tests (and ft-testkit's own tests don't pull in this file).

use ft_core::{hash::state_hash, validate_record_json};
use ft_testkit::{TestRepo, make_bug, make_epic, make_subtask, make_task};

#[test]
fn testkit_factories_pass_ft_core_schema_and_hash() {
    let tr = TestRepo::new().expect("create TestRepo");
    let task = make_task().title("sanity task").build();
    let parent_task_id = task.envelope.id.clone();
    let records = vec![
        task,
        make_epic().title("sanity epic").build(),
        make_subtask(parent_task_id).title("sanity sub").build(),
        make_bug().title("sanity bug").build(),
    ];

    for record in &records {
        // Schema validation: serialise → re-validate the JSON value.
        let value = serde_json::to_value(record).expect("serialize");
        validate_record_json(&value).expect("ft-testkit factory output passes ft-core schema");

        // Hash consistency: ft-core's canonical hash matches what the
        // factory embedded.
        let recomputed = state_hash(record).expect("recompute hash");
        assert_eq!(
            recomputed, record.envelope.state_hash,
            "ft-testkit factory `{:?}` produced an inconsistent state_hash",
            record.envelope.kind
        );
    }

    // Bonus: write through the assertion helper so the on-disk hash-verified
    // path is exercised (this also keeps the TestRepo binding live).
    let demo = &records[0];
    ft_testkit::assertions::write_record(&tr, demo).expect("write_record via ft-storage");
    ft_testkit::assert_record_exists(&tr, &demo.envelope.id);
    ft_testkit::assert_hash_consistent(&tr, &demo.envelope.id);
}
