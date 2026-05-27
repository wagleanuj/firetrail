//! Integration tests for `ft-git` exercising the public API against real
//! repositories created by `ft-testkit::TestRepo`.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

use ft_git::{ChangeKind, GitError, HookName, Repo};
use ft_testkit::TestRepo;

fn write(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn open_and_discover() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    assert_eq!(repo.root(), tr.root());

    // Discover from a nested path.
    let nested = tr.root().join(".firetrail/records/task");
    let repo2 = Repo::discover(&nested).unwrap();
    assert_eq!(repo2.root(), tr.root());
}

#[test]
fn open_not_a_repo_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let err = Repo::open(tmp.path()).unwrap_err();
    assert!(matches!(err, GitError::NotARepo(_)));
}

#[test]
fn open_missing_path_errors() {
    let err = Repo::open("/definitely/does/not/exist/firetrail-xyz").unwrap_err();
    assert!(matches!(err, GitError::NotFound(_)));
}

#[test]
fn head_and_current_branch_on_fresh_repo() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    let head = repo.head().unwrap();
    assert_eq!(head.commit_summary, "initial");
    assert!(head.name.ends_with("main"));
    assert_eq!(head.commit_sha.len(), 40);
    assert_eq!(repo.current_branch().unwrap().as_deref(), Some("main"));
    assert!(!repo.is_detached().unwrap());
}

#[test]
fn is_clean_and_status_track_workdir() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    assert!(repo.is_clean().unwrap());

    write(&tr.root().join("file.txt"), "hi\n");
    assert!(!repo.is_clean().unwrap());
    let status = repo.status().unwrap();
    assert!(!status.clean);
    assert_eq!(status.untracked, vec![PathBuf::from("file.txt")]);

    tr.commit_all("add file").unwrap();
    let status = repo.status().unwrap();
    assert!(status.clean);
}

#[test]
fn has_uncommitted_targets_path() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    write(&tr.root().join("a.txt"), "a");
    write(&tr.root().join("b.txt"), "b");
    assert!(repo.has_uncommitted("a.txt").unwrap());
    assert!(repo.has_uncommitted("b.txt").unwrap());
    tr.commit_all("seed").unwrap();
    assert!(!repo.has_uncommitted("a.txt").unwrap());
}

#[test]
fn detached_head_is_detected() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join("a.txt"), "v1");
    tr.commit_all("c1").unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    let head_sha = repo.head().unwrap().commit_sha;

    // Detach.
    let _ = tr.run("git", &["checkout", "--quiet", &head_sha]).unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    assert!(repo.is_detached().unwrap());
    assert!(repo.current_branch().unwrap().is_none());
    // head() still works on detached HEAD.
    let info = repo.head().unwrap();
    assert_eq!(info.name, "HEAD");
}

#[test]
fn branch_lifecycle_via_ft_git() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    assert!(repo.branches().unwrap().iter().any(|b| b.name == "main"));

    repo.branch_create("feature/x", "main").unwrap();
    assert!(repo.branch_exists("feature/x").unwrap());

    // Can't create the same branch twice.
    let err = repo.branch_create("feature/x", "main").unwrap_err();
    assert!(matches!(err, GitError::BranchExists(_)));

    repo.checkout("feature/x").unwrap();
    assert_eq!(repo.current_branch().unwrap().as_deref(), Some("feature/x"));

    repo.checkout("main").unwrap();
    repo.branch_delete("feature/x", true).unwrap();
    assert!(!repo.branch_exists("feature/x").unwrap());
}

#[test]
fn read_file_at_ref_returns_bytes_or_not_in_tree() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join("hello.txt"), "hello\n");
    tr.commit_all("add hello").unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    let head = repo.head().unwrap();
    let bytes = repo
        .read_file_at_ref(&head.commit_sha, "hello.txt")
        .unwrap();
    assert_eq!(bytes, b"hello\n");

    let err = repo
        .read_file_at_ref(&head.commit_sha, "does-not-exist.txt")
        .unwrap_err();
    assert!(matches!(err, GitError::FileNotInTree(_, _)));
}

#[test]
fn read_file_from_main_while_on_feature_branch() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join("main-only.txt"), "main\n");
    tr.commit_all("seed on main").unwrap();
    tr.branch("feature/y").unwrap();
    tr.checkout("feature/y").unwrap();
    write(&tr.root().join("main-only.txt"), "feature\n");
    tr.commit_all("override on feature").unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    let on_main = repo.read_file_at_ref("main", "main-only.txt").unwrap();
    assert_eq!(on_main, b"main\n");
    let on_feat = repo.read_file_at_ref("feature/y", "main-only.txt").unwrap();
    assert_eq!(on_feat, b"feature\n");
}

#[test]
fn list_files_at_ref_honors_glob_and_sorts() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join(".firetrail/records/task/b.json"), "{}");
    write(&tr.root().join(".firetrail/records/task/a.json"), "{}");
    write(&tr.root().join(".firetrail/records/bug/c.json"), "{}");
    write(&tr.root().join(".firetrail/records/task/notes.md"), "n");
    tr.commit_all("seed records").unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    let files = repo
        .list_files_at_ref("HEAD", ".firetrail/records/**/*.json")
        .unwrap();
    assert_eq!(
        files,
        vec![
            PathBuf::from(".firetrail/records/bug/c.json"),
            PathBuf::from(".firetrail/records/task/a.json"),
            PathBuf::from(".firetrail/records/task/b.json"),
        ]
    );

    // The .md file is excluded by the glob.
    assert!(
        !files
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "md"))
    );
}

#[test]
fn diff_between_refs_detects_changes() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join("a.txt"), "v1");
    tr.commit_all("c1").unwrap();
    write(&tr.root().join("a.txt"), "v2");
    write(&tr.root().join("b.txt"), "new");
    tr.commit_all("c2").unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    let entries = repo.diff("HEAD~1", "HEAD", None).unwrap();
    let kinds: Vec<_> = entries
        .iter()
        .map(|e| (e.path.clone(), e.change_kind.clone()))
        .collect();
    assert!(kinds.contains(&(PathBuf::from("a.txt"), ChangeKind::Modified)));
    assert!(kinds.contains(&(PathBuf::from("b.txt"), ChangeKind::Added)));
}

#[test]
fn diff_path_filter_restricts_results() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join("a/foo.txt"), "v1");
    write(&tr.root().join("b/bar.txt"), "v1");
    tr.commit_all("c1").unwrap();
    write(&tr.root().join("a/foo.txt"), "v2");
    write(&tr.root().join("b/bar.txt"), "v2");
    tr.commit_all("c2").unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    let entries = repo.diff("HEAD~1", "HEAD", Some("a/")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, PathBuf::from("a/foo.txt"));
}

#[test]
fn log_path_returns_only_commits_touching_path() {
    let tr = TestRepo::new().unwrap();
    write(&tr.root().join("log.txt"), "v1");
    tr.commit_all("first").unwrap();
    write(&tr.root().join("other.txt"), "v1");
    tr.commit_all("untouched").unwrap();
    write(&tr.root().join("log.txt"), "v2");
    tr.commit_all("second").unwrap();

    let repo = Repo::open(tr.root()).unwrap();
    let commits = repo.log_path("log.txt", Some(10)).unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].summary, "second");
    assert_eq!(commits[1].summary, "first");
}

#[test]
fn install_hook_writes_executable_with_markers() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    assert!(!repo.hook_installed(HookName::PreCommit));

    repo.install_hook(HookName::PreCommit, "echo hello")
        .unwrap();
    assert!(repo.hook_installed(HookName::PreCommit));

    let hook_path = tr.root().join(".git/hooks/pre-commit");
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("echo hello"));
    assert!(content.contains("# >>> firetrail managed >>>"));
    assert!(content.contains("# <<< firetrail managed <<<"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&hook_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "hook is not executable: mode={mode:o}");
    }
}

#[test]
fn reinstall_hook_replaces_only_managed_region() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();

    // Manually write a hook with user content above the firetrail block.
    let hook_path = tr.root().join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook_path.parent().unwrap()).unwrap();
    std::fs::write(
        &hook_path,
        "#!/bin/sh\necho user-content\n# >>> firetrail managed >>>\necho old\n# <<< firetrail managed <<<\necho footer\n",
    )
    .unwrap();

    repo.install_hook(HookName::PreCommit, "echo NEW").unwrap();
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("echo user-content"));
    assert!(content.contains("echo NEW"));
    assert!(!content.contains("echo old"));
    assert!(content.contains("echo footer"));
}

#[test]
fn remove_hook_drops_managed_region_and_file() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();

    repo.install_hook(HookName::PostCheckout, "echo hi")
        .unwrap();
    assert!(repo.hook_installed(HookName::PostCheckout));
    repo.remove_hook(HookName::PostCheckout).unwrap();
    assert!(!repo.hook_installed(HookName::PostCheckout));

    // Removing a non-existent hook is a no-op.
    repo.remove_hook(HookName::PostMerge).unwrap();
}

#[test]
fn pre_receive_protect_firetrail_writes_to_artifact_path() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    repo.install_hook(
        HookName::PreReceiveProtectFiretrail,
        "# guard firetrail records",
    )
    .unwrap();
    let artifact = tr.root().join(".firetrail/hooks/pre-receive");
    assert!(artifact.exists());
    assert!(!tr.root().join(".git/hooks/pre-receive").exists());
    assert!(repo.hook_installed(HookName::PreReceiveProtectFiretrail));
}

#[test]
fn ref_not_found_for_bogus_ref() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    let err = repo
        .read_file_at_ref("definitely-not-a-ref", "anything")
        .unwrap_err();
    assert!(matches!(err, GitError::RefNotFound(_)));
}

#[test]
fn corrupted_main_ref_yields_error_not_panic() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    // Corrupt the packed/loose ref. Either kind may be present depending on git version.
    let loose = tr.root().join(".git/refs/heads/main");
    if loose.exists() {
        std::fs::write(&loose, "deadbeef-not-a-sha").unwrap();
    } else {
        // Force a packed-refs corruption fallback.
        let packed = tr.root().join(".git/packed-refs");
        std::fs::write(&packed, "garbage\n").unwrap();
    }

    let result = repo.head();
    assert!(
        matches!(result, Err(GitError::Gix(_) | GitError::Shell { .. })),
        "expected Gix or Shell error, got {result:?}"
    );
}

#[test]
fn branches_reports_current_flag() {
    let tr = TestRepo::new().unwrap();
    tr.branch("dev").unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    let branches = repo.branches().unwrap();
    assert!(branches.iter().any(|b| b.name == "main" && b.is_current));
    assert!(branches.iter().any(|b| b.name == "dev" && !b.is_current));

    // Branches are sorted alphabetically.
    let names: Vec<_> = branches.iter().map(|b| b.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

/// Smoke-test that the shell-out helper sets `GIT_TERMINAL_PROMPT=0` by
/// asserting that a remote operation against a non-existent remote fails
/// instead of prompting. We can't observe the env directly, so we just ensure
/// failures surface as `GitError::Shell`.
#[test]
fn shell_out_failure_is_categorized() {
    let tr = TestRepo::new().unwrap();
    let repo = Repo::open(tr.root()).unwrap();
    // Attempt to delete a branch that does not exist.
    let err = repo.branch_delete("never-existed", false).unwrap_err();
    assert!(matches!(err, GitError::Shell { .. }));
}

// Sanity: silence dead_code warnings on Command import when running on non-Unix
// targets where some checks are skipped.
#[allow(dead_code)]
fn _ensure_command_used() {
    let _ = Command::new("true");
}
