//! Property tests for [`Repo::list_files_at_ref`] glob behavior.
//!
//! Seeds a tree with random filenames matching one of a handful of patterns
//! and asserts:
//!
//! - The result of `list_files_at_ref` is a subset of the seeded files.
//! - Every returned path matches the requested glob.
//! - The result is sorted.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use ft_git::Repo;
use ft_testkit::TestRepo;
use globset::Glob;
use proptest::prelude::*;

fn filename_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("alpha.json".to_string()),
        Just("beta.json".to_string()),
        Just("gamma.md".to_string()),
        Just("delta.txt".to_string()),
        Just("nested.json".to_string()),
    ]
}

fn directory_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(".firetrail/records/task".to_string()),
        Just(".firetrail/records/bug".to_string()),
        Just(".firetrail/records/decision".to_string()),
        Just("notes".to_string()),
    ]
}

fn glob_strategy() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just(".firetrail/records/**/*.json"),
        Just(".firetrail/records/task/*.json"),
        Just("**/*.md"),
        Just("**/*.json"),
        Just("notes/*"),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        .. ProptestConfig::default()
    })]

    #[test]
    fn list_files_at_ref_glob_is_consistent(
        files in prop::collection::vec((directory_strategy(), filename_strategy()), 1..8),
        pattern in glob_strategy(),
    ) {
        let tr = TestRepo::new().unwrap();

        // Seed unique paths.
        let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
        for (dir, name) in &files {
            let full = PathBuf::from(dir).join(name);
            let abs = tr.root().join(&full);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(&abs, b"{}").unwrap();
            paths.insert(full);
        }
        tr.commit_all("seed").unwrap();

        let repo = Repo::open(tr.root()).unwrap();
        let listed = repo.list_files_at_ref("HEAD", pattern).unwrap();

        // Compile the same glob to verify the matcher's view of truth.
        let matcher = Glob::new(pattern).unwrap().compile_matcher();
        let expected: Vec<PathBuf> = paths
            .iter()
            .filter(|p| matcher.is_match(p))
            .cloned()
            .collect();

        prop_assert_eq!(listed.clone(), expected);

        // Sorted check (vacuously true if 0 or 1 entries).
        let mut sorted = listed.clone();
        sorted.sort();
        prop_assert_eq!(listed, sorted);
    }
}
