//! Isolated tempdir-backed Firetrail workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use tempfile::TempDir;

use crate::error::TestKitError;

/// Storage backend selection for the test workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StorageMode {
    /// Records stored as JSON files under `.firetrail/records/` (M1 default).
    #[default]
    Embedded,
    /// Records served by an external storage URL.
    ///
    /// Declared for forward compatibility; M1 callers always use `Embedded`.
    External(String),
}

/// Configuration for [`TestRepo::with_config`].
#[derive(Debug, Clone)]
pub struct TestRepoConfig {
    /// Storage backend.
    pub storage_mode: StorageMode,
    /// Whether `ft-identity` should reject unknown identities.
    pub strict_identity: bool,
    /// `user.email` set on the git repo.
    pub author_email: String,
    /// `user.name` set on the git repo.
    pub author_name: String,
    /// Whether to install Firetrail git hooks (M1: no-op).
    pub install_hooks: bool,
}

impl Default for TestRepoConfig {
    fn default() -> Self {
        Self {
            storage_mode: StorageMode::Embedded,
            strict_identity: false,
            author_email: "tester@firetrail.test".to_string(),
            author_name: "Firetrail Tester".to_string(),
            install_hooks: false,
        }
    }
}

/// Captured stdout/stderr/exit-status of a spawned process.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    /// Captured stdout, lossy-UTF8 decoded.
    pub stdout: String,
    /// Captured stderr, lossy-UTF8 decoded.
    pub stderr: String,
    /// Final exit status.
    pub status: ExitStatus,
}

impl CmdOutput {
    /// Whether the command exited with code 0.
    #[must_use]
    pub fn success(&self) -> bool {
        self.status.success()
    }
}

/// An isolated Firetrail workspace backed by a tempdir.
///
/// Dropping the `TestRepo` removes the tempdir.
#[derive(Debug)]
pub struct TestRepo {
    root: PathBuf,
    config: TestRepoConfig,
    // Held for RAII cleanup; field name leading underscore retained per spec.
    _tempdir: TempDir,
}

impl TestRepo {
    /// Create a fresh empty repo with `git init` and a minimal `.firetrail/`.
    pub fn new() -> Result<Self, TestKitError> {
        Self::with_config(TestRepoConfig::default())
    }

    /// Create a repo with a custom configuration.
    pub fn with_config(config: TestRepoConfig) -> Result<Self, TestKitError> {
        if config.author_email.trim().is_empty() {
            return Err(TestKitError::Config("author_email is empty".into()));
        }
        if config.author_name.trim().is_empty() {
            return Err(TestKitError::Config("author_name is empty".into()));
        }

        let tempdir = TempDir::new()?;
        let root = tempdir.path().to_path_buf();

        // git init
        run_git(&root, &["init", "--quiet", "--initial-branch=main"])
            .or_else(|_| run_git(&root, &["init", "--quiet"]))?;
        run_git(&root, &["config", "user.email", &config.author_email])?;
        run_git(&root, &["config", "user.name", &config.author_name])?;
        run_git(&root, &["config", "commit.gpgsign", "false"])?;

        // Initial empty commit so downstream branch/checkout work.
        run_git(
            &root,
            &["commit", "--allow-empty", "--quiet", "-m", "initial"],
        )?;

        // Ensure HEAD branch is "main" when supported.
        let _ = run_git(&root, &["branch", "-M", "main"]);

        // Minimal .firetrail/ skeleton.
        let firetrail = root.join(".firetrail");
        std::fs::create_dir_all(firetrail.join("records"))?;
        for kind in [
            "task", "epic", "subtask", "bug", "incident", "finding", "runbook", "decision",
            "gotcha", "memory",
        ] {
            std::fs::create_dir_all(firetrail.join("records").join(kind))?;
        }

        Ok(Self {
            root,
            config,
            _tempdir: tempdir,
        })
    }

    /// Absolute path of the repo root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Absolute path of `.firetrail/`.
    #[must_use]
    pub fn firetrail_dir(&self) -> PathBuf {
        self.root.join(".firetrail")
    }

    /// Snapshot of the active configuration.
    #[must_use]
    pub fn config(&self) -> &TestRepoConfig {
        &self.config
    }

    /// Commit currently staged changes.
    pub fn commit(&self, message: &str) -> Result<(), TestKitError> {
        run_git(&self.root, &["commit", "--quiet", "-m", message])?;
        Ok(())
    }

    /// Stage everything under the repo and commit.
    pub fn commit_all(&self, message: &str) -> Result<(), TestKitError> {
        run_git(&self.root, &["add", "-A"])?;
        run_git(&self.root, &["commit", "--quiet", "-m", message])?;
        Ok(())
    }

    /// Create a branch from current HEAD.
    pub fn branch(&self, name: &str) -> Result<(), TestKitError> {
        run_git(&self.root, &["branch", name])?;
        Ok(())
    }

    /// Check out a branch.
    pub fn checkout(&self, name: &str) -> Result<(), TestKitError> {
        run_git(&self.root, &["checkout", "--quiet", name])?;
        Ok(())
    }

    /// Current branch name.
    pub fn current_branch(&self) -> Result<String, TestKitError> {
        let out = run_git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        Ok(out.trim().to_string())
    }

    /// Run a shell command in the repo root.
    pub fn run(&self, cmd: &str, args: &[&str]) -> Result<CmdOutput, TestKitError> {
        let output = Command::new(cmd)
            .args(args)
            .current_dir(&self.root)
            .output()
            .map_err(|e| TestKitError::Cmd(format!("spawn `{cmd}` failed: {e}")))?;
        Ok(CmdOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: output.status,
        })
    }

    /// Run the `firetrail` binary against this repo.
    ///
    /// Locates the binary via `option_env!("CARGO_BIN_EXE_firetrail")`. At M1
    /// the binary is still a stub; callers that depend on real behavior should
    /// gate themselves with `#[ignore]` until ft-cli lands.
    ///
    /// # Errors
    ///
    /// Returns [`TestKitError::Cmd`] if the binary was not built when this
    /// crate was compiled (i.e. ft-cli is not in the dependency graph of the
    /// test target).
    pub fn firetrail(&self, args: &[&str]) -> Result<CmdOutput, TestKitError> {
        let Some(bin) = option_env!("CARGO_BIN_EXE_firetrail") else {
            return Err(TestKitError::Cmd(
                "firetrail binary not available: CARGO_BIN_EXE_firetrail not set at compile time \
                 (add ft-cli as a build-dependency or run the calling test target with ft-cli \
                 built)"
                    .to_string(),
            ));
        };
        let str_args: Vec<&str> = args.to_vec();
        self.run(bin, &str_args)
    }
}

/// Helper: run a git subcommand, capturing stdout. Returns stdout on success,
/// or a [`TestKitError::Git`] containing stderr on failure.
fn run_git(root: &Path, args: &[&str]) -> Result<String, TestKitError> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| TestKitError::Git(format!("spawn git failed: {e}")))?;
    if !out.status.success() {
        return Err(TestKitError::Git(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
