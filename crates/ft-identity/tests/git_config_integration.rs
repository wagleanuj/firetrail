//! Integration tests: resolve identity through a real `git config` in a
//! tempdir-backed [`ft_testkit::TestRepo`].
//!
//! These tests exercise the only resolution branch that actually shells out:
//! step 4, `git config user.email`. The git binary must be installed on the
//! host; if it is not, `TestRepo::new` fails and the test is skipped.

use ft_identity::{DefaultResolver, IdentityResolver, MockEnv, ResolutionSource, SourceResult};
use ft_testkit::repo::{TestRepo, TestRepoConfig};

fn try_repo(cfg: TestRepoConfig) -> Option<TestRepo> {
    TestRepo::with_config(cfg).ok()
}

#[test]
fn resolves_from_git_config_user_email() {
    let Some(repo) = try_repo(TestRepoConfig {
        author_email: "alice@example.com".into(),
        author_name: "Alice".into(),
        ..TestRepoConfig::default()
    }) else {
        eprintln!("git unavailable; skipping");
        return;
    };

    let resolver =
        DefaultResolver::with_env(repo.root().to_path_buf(), false, Box::new(MockEnv::new()));
    let id = resolver
        .resolve()
        .expect("git config user.email should resolve");
    assert_eq!(id.as_str(), "alice@example.com");
}

#[test]
fn env_var_beats_git_config() {
    let Some(repo) = try_repo(TestRepoConfig {
        author_email: "git@example.com".into(),
        author_name: "Git".into(),
        ..TestRepoConfig::default()
    }) else {
        return;
    };

    let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "env@example.com");
    let resolver = DefaultResolver::with_env(repo.root().to_path_buf(), false, Box::new(env));
    let id = resolver.resolve().unwrap();
    assert_eq!(id.as_str(), "env@example.com");
}

#[test]
fn local_identity_file_beats_git_config() {
    let Some(repo) = try_repo(TestRepoConfig {
        author_email: "git@example.com".into(),
        author_name: "Git".into(),
        ..TestRepoConfig::default()
    }) else {
        return;
    };

    let identity_path = repo.firetrail_dir().join("identity.yml");
    std::fs::write(&identity_path, "email: file@example.com\n").unwrap();

    let resolver =
        DefaultResolver::with_env(repo.root().to_path_buf(), false, Box::new(MockEnv::new()));
    let id = resolver.resolve().unwrap();
    assert_eq!(id.as_str(), "file@example.com");
}

#[test]
fn trace_reports_git_config_found_when_only_source() {
    let Some(repo) = try_repo(TestRepoConfig {
        author_email: "alice@example.com".into(),
        author_name: "Alice".into(),
        ..TestRepoConfig::default()
    }) else {
        return;
    };

    let resolver =
        DefaultResolver::with_env(repo.root().to_path_buf(), false, Box::new(MockEnv::new()));
    let trace = resolver.resolve_with_trace().unwrap();

    let git_check = trace
        .sources_checked
        .iter()
        .find(|c| c.source == ResolutionSource::GitConfig)
        .expect("trace should include git config");
    assert!(
        matches!(&git_check.result, SourceResult::Found(v) if v == "alice@example.com"),
        "expected Found(alice@example.com), got {:?}",
        git_check.result
    );
}
