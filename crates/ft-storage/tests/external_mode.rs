//! Integration tests for [`ft_storage::ExternalStorage`] and friends.

use std::path::Path;
use std::process::Command;

use ft_core::Identity;
use ft_storage::{
    EmbeddedStorage, ExternalConfig, ExternalStorage, Storage, StorageMode, SyncPolicy,
    open_for_workspace, sync_status, validate_external_references,
};
use ft_testkit::{TestRepo, make_task};

/// Create a bare repository to act as the "remote" data repo, seeded with an
/// initial commit so `git clone` succeeds.
fn make_bare_remote(label: &str) -> tempfile::TempDir {
    let dir = tempfile::TempDir::with_prefix(format!("ft-remote-{label}-")).unwrap();
    // Initialize bare repo.
    Command::new("git")
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .current_dir(dir.path())
        .status()
        .or_else(|_| {
            Command::new("git")
                .args(["init", "--bare", "--quiet"])
                .current_dir(dir.path())
                .status()
        })
        .unwrap();
    // Seed: clone to a scratch worktree, commit an empty README, push.
    let scratch = tempfile::TempDir::with_prefix("ft-remote-seed-").unwrap();
    let url = format!("file://{}", dir.path().display());
    run(
        scratch.path().parent().unwrap(),
        &[
            "git",
            "clone",
            "--quiet",
            &url,
            &scratch.path().to_string_lossy(),
        ],
    );
    run(
        scratch.path(),
        &["git", "config", "user.email", "seed@firetrail.test"],
    );
    run(scratch.path(), &["git", "config", "user.name", "Seed"]);
    run(
        scratch.path(),
        &["git", "config", "commit.gpgsign", "false"],
    );
    std::fs::write(scratch.path().join("README.md"), "data repo\n").unwrap();
    run(scratch.path(), &["git", "add", "README.md"]);
    run(scratch.path(), &["git", "commit", "--quiet", "-m", "seed"]);
    // Push to the bare; tolerate either default-branch name.
    if Command::new("git")
        .args(["push", "--quiet", "origin", "main"])
        .current_dir(scratch.path())
        .status()
        .unwrap()
        .success()
    {
        // ok
    } else {
        run(
            scratch.path(),
            &["git", "push", "--quiet", "origin", "HEAD:main"],
        );
    }
    dir
}

fn run(cwd: &Path, parts: &[&str]) {
    let status = Command::new(parts[0])
        .args(&parts[1..])
        .current_dir(cwd)
        .status()
        .expect("spawn");
    assert!(status.success(), "command failed: {parts:?}");
}

fn remote_url(remote: &tempfile::TempDir) -> String {
    format!("file://{}", remote.path().display())
}

fn external_config(url: String) -> ExternalConfig {
    ExternalConfig {
        data_repo_url: url,
        default_branch: "main".to_string(),
        sync_policy: SyncPolicy::Loose,
    }
}

#[test]
fn open_clones_and_round_trips_through_push_pull() {
    let remote = make_bare_remote("rt");

    // Writer side.
    let writer_ws = TestRepo::new().unwrap();
    let storage = ExternalStorage::open(writer_ws.root(), &external_config(remote_url(&remote)))
        .expect("open writer");

    let record = make_task().title("from-writer").build();
    storage.write(&record).unwrap();
    storage.push().expect("push");

    // Reader side: a separate sibling clone of the same data repo.
    let reader_ws = TestRepo::new().unwrap();
    let reader = ExternalStorage::open(reader_ws.root(), &external_config(remote_url(&remote)))
        .expect("open reader");
    // The clone managed by `open()` already fetched; pull just to be sure.
    reader.pull().expect("pull");
    let back = reader
        .read(&record.envelope.id)
        .expect("reader sees record");
    assert_eq!(back.envelope.title, "from-writer");
}

#[test]
fn sync_status_reports_ahead_after_local_write() {
    let remote = make_bare_remote("status");
    let ws = TestRepo::new().unwrap();
    let storage =
        ExternalStorage::open(ws.root(), &external_config(remote_url(&remote))).expect("open");

    // Clean clone: nothing ahead, nothing behind.
    let initial = sync_status(&storage).expect("status");
    assert_eq!(initial.ahead, 0, "fresh clone is not ahead: {initial:?}");

    let record = make_task().title("local-only").build();
    storage.write(&record).unwrap();

    let after = sync_status(&storage).expect("status");
    assert!(
        after.ahead >= 1,
        "expected ahead >= 1 after a write commit, got {after:?}"
    );
}

#[test]
fn open_for_workspace_dispatches_to_external() {
    let remote = make_bare_remote("dispatch");
    let ws = TestRepo::new().unwrap();
    // Write a config.yml that picks external mode.
    let cfg = format!(
        "storage:\n  mode: external\n  data_repo_url: {}\n  default_branch: main\n  sync_policy: loose\n",
        remote_url(&remote)
    );
    std::fs::write(ws.root().join(".firetrail").join("config.yml"), cfg).unwrap();

    let storage = open_for_workspace(ws.root()).expect("open_for_workspace");

    let record = make_task().title("via dispatch").build();
    storage.write(&record).unwrap();
    let back = storage.read(&record.envelope.id).unwrap();
    assert_eq!(back.envelope.title, "via dispatch");

    // The actual records tree should live under the cache clone, not in the
    // code repo's .firetrail/records.
    let cache_records = ws
        .root()
        .join(ExternalStorage::CLONE_SUBPATH)
        .join(".firetrail/records");
    assert!(cache_records.exists(), "cache records dir exists");
}

#[test]
fn open_for_workspace_dispatches_to_embedded() {
    let ws = TestRepo::new().unwrap();
    std::fs::write(
        ws.root().join(".firetrail").join("config.yml"),
        "storage:\n  mode: embedded\n",
    )
    .unwrap();
    let storage = open_for_workspace(ws.root()).expect("open");
    let r = make_task().title("via embedded").build();
    storage.write(&r).unwrap();
    let back = storage.read(&r.envelope.id).unwrap();
    assert_eq!(back.envelope.title, "via embedded");
}

#[test]
fn storage_mode_from_workspace_parses_external() {
    let ws = TestRepo::new().unwrap();
    std::fs::write(
        ws.root().join(".firetrail").join("config.yml"),
        "storage:\n  mode: external\n  data_repo_url: file:///tmp/x\n",
    )
    .unwrap();
    let mode = StorageMode::from_workspace(ws.root()).unwrap();
    assert!(matches!(mode, StorageMode::External { .. }));
}

#[test]
fn validate_external_references_flags_unknown_record() {
    // Code repo with a commit referencing a record id that does NOT exist in
    // the data storage.
    let code = TestRepo::new().unwrap();
    let code_repo = ft_git::Repo::open(code.root()).unwrap();

    // Make a fresh commit on a feature branch that references a fake record.
    code.branch("feat").unwrap();
    code.checkout("feat").unwrap();
    let fake_id = ft_core::RecordId::mint(
        ft_core::RecordKind::Task,
        &Identity::new("ref@firetrail.test").unwrap(),
    );
    std::fs::write(code.root().join("CHANGELOG.md"), "feature work\n").unwrap();
    code.run("git", &["add", "CHANGELOG.md"]).unwrap();
    let msg = format!("firetrail-closes: {}", fake_id.as_str());
    code.run("git", &["commit", "--quiet", "-m", &msg]).unwrap();

    // Data storage with NO records.
    let data_ws = TestRepo::new().unwrap();
    let data = EmbeddedStorage::open(data_ws.root()).unwrap();

    let v = validate_external_references(&code_repo, &data, "main", "feat");
    assert!(!v.is_empty(), "expected at least one violation, got {v:?}");
    assert!(v.iter().any(|x| x.record_id.as_str() == fake_id.as_str()));
}

#[test]
fn validate_external_references_accepts_known_record() {
    let code = TestRepo::new().unwrap();
    let code_repo = ft_git::Repo::open(code.root()).unwrap();

    // Write the record into the data storage first.
    let data_ws = TestRepo::new().unwrap();
    let data = EmbeddedStorage::open(data_ws.root()).unwrap();
    let record = make_task().title("real").build();
    data.write(&record).unwrap();

    // Code commit references the *existing* record.
    code.branch("feat").unwrap();
    code.checkout("feat").unwrap();
    std::fs::write(code.root().join("notes.md"), "yes\n").unwrap();
    code.run("git", &["add", "notes.md"]).unwrap();
    let msg = format!("firetrail-closes: {}", record.envelope.id.as_str());
    code.run("git", &["commit", "--quiet", "-m", &msg]).unwrap();

    let v = validate_external_references(&code_repo, &data, "main", "feat");
    assert!(v.is_empty(), "expected no violations, got {v:?}");
}
