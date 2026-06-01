//! Integration tests for `ft_ops::files::list_files` — the tracked-path lister
//! the ft-ui file-path autocomplete calls.
//!
//! Seeds a couple of committed files so `HEAD` has a tree (mirrors the seeding
//! pattern in the ft-git/ft-ops tests), then exercises the prefix filter,
//! `dirs_only` collapse, and `limit` truncation.

use ft_ops::Workspace;
use ft_ops::files::list_files;
use ft_testkit::TestRepo;

fn fixture() -> (TestRepo, Workspace) {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).expect("mkdir .firetrail");
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .expect("write config.yml");

    // Seed a tree under HEAD: a handful of tracked files in nested dirs.
    let root = tr.root();
    for rel in [
        "crates/ft-cli/src/main.rs",
        "crates/ft-cli/src/lib.rs",
        "crates/ft-ui/src/server.rs",
        "docs/README.md",
        "README.md",
    ] {
        let abs = root.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).expect("mkdir");
        std::fs::write(&abs, b"// seed\n").expect("write file");
    }
    tr.commit_all("seed files").expect("commit");

    let ws = Workspace::open(tr.root()).expect("open workspace");
    (tr, ws)
}

#[test]
fn prefix_filters_tracked_files() {
    let (_tr, ws) = fixture();
    let out = list_files(&ws, "crates/ft-cli", false, 50).expect("list");
    assert_eq!(
        out,
        vec![
            "crates/ft-cli/src/lib.rs".to_string(),
            "crates/ft-cli/src/main.rs".to_string(),
        ],
    );
}

#[test]
fn empty_prefix_lists_all_tracked_files() {
    let (_tr, ws) = fixture();
    let out = list_files(&ws, "", false, 50).expect("list");
    // All five seeded files (config.yml under .firetrail is also tracked).
    assert!(out.contains(&"README.md".to_string()));
    assert!(out.contains(&"crates/ft-cli/src/main.rs".to_string()));
    assert!(out.contains(&"crates/ft-ui/src/server.rs".to_string()));
}

#[test]
fn prefix_is_case_insensitive() {
    let (_tr, ws) = fixture();
    let out = list_files(&ws, "CRATES/FT-UI", false, 50).expect("list");
    assert_eq!(out, vec!["crates/ft-ui/src/server.rs".to_string()]);
}

#[test]
fn dirs_only_collapses_to_distinct_directory_prefixes() {
    let (_tr, ws) = fixture();
    let out = list_files(&ws, "crates", true, 50).expect("list");
    // Distinct ancestor directories under `crates`, no file paths.
    assert_eq!(
        out,
        vec![
            "crates".to_string(),
            "crates/ft-cli".to_string(),
            "crates/ft-cli/src".to_string(),
            "crates/ft-ui".to_string(),
            "crates/ft-ui/src".to_string(),
        ],
    );
    // No file paths leak through.
    assert!(
        !out.iter()
            .any(|p| std::path::Path::new(p).extension().is_some())
    );
}

#[test]
fn limit_truncates_results() {
    let (_tr, ws) = fixture();
    let out = list_files(&ws, "", false, 2).expect("list");
    assert_eq!(out.len(), 2);
}

#[test]
fn limit_is_clamped_to_at_least_one() {
    let (_tr, ws) = fixture();
    let out = list_files(&ws, "", false, 0).expect("list");
    assert_eq!(out.len(), 1);
}
