//! Property tests for path encoding.
//!
//! `path_for(id)` is the canonical inverse of `parse_path_to_id`: writing a
//! file at `path_for(id)` and reading the components back must yield the
//! original `(kind, lowercase-id)` pair.

use ft_core::{Identity, RecordId, RecordKind};
use ft_storage::{EmbeddedStorage, Storage};
use ft_testkit::TestRepo;
use proptest::prelude::*;

fn record_kinds() -> impl Strategy<Value = RecordKind> {
    prop_oneof![
        Just(RecordKind::Task),
        Just(RecordKind::Epic),
        Just(RecordKind::Subtask),
        Just(RecordKind::Bug),
        Just(RecordKind::Incident),
        Just(RecordKind::Finding),
        Just(RecordKind::Runbook),
        Just(RecordKind::Decision),
        Just(RecordKind::Gotcha),
        Just(RecordKind::Memory),
    ]
}

proptest! {
    #[test]
    fn path_for_round_trips(kind in record_kinds()) {
        let tr = TestRepo::new().unwrap();
        let s = EmbeddedStorage::open(tr.root()).unwrap();
        let id = RecordId::mint(kind, &Identity::new("alice@example.com").unwrap());

        let path = s.path_for(&id);
        // Filename is the lowercase id + ".json".
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let expected = format!("{}.json", id.as_str().to_lowercase());
        prop_assert_eq!(&fname, &expected);

        // Parent directory matches the kind dir.
        let parent = path.parent().unwrap().file_name().unwrap().to_string_lossy().to_string();
        prop_assert_eq!(parent, ft_storage::kind_dir(kind));

        // No uppercase anywhere in the filename.
        prop_assert!(!fname.chars().any(|c| c.is_ascii_uppercase()));
    }
}
