//! External storage mode (ADR-0006).
//!
//! In external mode, records live in a SEPARATE git repository — the *data
//! repo* — rather than alongside code in the workspace repo. Firetrail clones
//! the data repo to `.firetrail/cache/data-repo` inside the workspace and
//! reads / writes against that clone. Cross-repo references are validated by
//! [`validate_external_references`] and enforced in CI (ADR-0010).
//!
//! ## Sync semantics (M5 — `loose` policy)
//!
//! - [`ExternalStorage::write`] auto-commits each write to the local clone
//!   with a stable commit message (`firetrail: write <id>`). The clone is the
//!   source of truth; pushing is an explicit step.
//! - [`ExternalStorage::pull`] performs `git fetch` + fast-forward of the
//!   tracking branch. Diverged histories return a merge-required error rather
//!   than silently creating merge commits — operators resolve manually.
//! - [`ExternalStorage::push`] runs `git push` against the configured remote
//!   for the local branch. Failures (non-fast-forward, auth) surface as
//!   [`StorageError`] variants.
//!
//! Strict / auto-sync policies are deferred (see ADR-0006); only [`SyncPolicy::Loose`]
//! is wired up in M5.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use ft_core::{Record, RecordId, RecordKind, state_hash, validate_record_json};
use ft_git::Repo;

use crate::filter::StorageFilter;
use crate::storage::Storage;
use crate::{RECORDS_DIR, StorageError, kind_dir};

/// Sync policy governing how the local clone reconciles with the remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncPolicy {
    /// Eventually consistent. Writes commit locally; operator pushes when
    /// ready. Conflicting remote history requires manual resolution.
    #[default]
    Loose,
}

/// Static, operator-supplied configuration for an [`ExternalStorage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalConfig {
    /// URL of the remote data repository (`file://`, `ssh://`, `https://`, …).
    pub data_repo_url: String,
    /// Default branch to track on the remote (typically `main`).
    pub default_branch: String,
    /// Sync policy. Only [`SyncPolicy::Loose`] is implemented at M5.
    pub sync_policy: SyncPolicy,
}

impl Default for ExternalConfig {
    fn default() -> Self {
        Self {
            data_repo_url: String::new(),
            default_branch: "main".to_string(),
            sync_policy: SyncPolicy::Loose,
        }
    }
}

/// Snapshot of the local clone's relationship to its remote tracking branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    /// Commits present locally but not on the remote.
    pub ahead: usize,
    /// Commits present on the remote but not locally.
    pub behind: usize,
    /// Whether the local clone has uncommitted changes.
    pub dirty: bool,
}

/// External-mode storage. Records live in a separate cloned repository.
#[derive(Debug, Clone)]
pub struct ExternalStorage {
    workspace_root: PathBuf,
    clone_path: PathBuf,
    config: ExternalConfig,
    git: Arc<Repo>,
}

impl ExternalStorage {
    /// Relative path (within the workspace) of the data-repo clone.
    pub const CLONE_SUBPATH: &'static str = ".firetrail/cache/data-repo";

    /// Open external storage rooted at `workspace_root`.
    ///
    /// Ensures the clone at `.firetrail/cache/data-repo` exists: clones the
    /// configured remote if missing, otherwise fetches updates. The clone is
    /// treated as the source of truth for all subsequent reads and writes.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Io`] if the cache directory cannot be created.
    /// - [`StorageError::Git`] if the clone or fetch fails.
    pub fn open(workspace_root: &Path, config: &ExternalConfig) -> Result<Self, StorageError> {
        if config.data_repo_url.trim().is_empty() {
            return Err(StorageError::Invalid {
                path: workspace_root.join(".firetrail/config.yml"),
                reason: "external storage requires a non-empty data_repo_url".into(),
            });
        }
        let clone_path = workspace_root.join(Self::CLONE_SUBPATH);
        let git = ensure_data_repo_cloned(&config.data_repo_url, &clone_path)?;

        // Make sure the records tree exists in the clone (init on first use).
        let records_root = clone_path.join(RECORDS_DIR);
        if !records_root.is_dir() {
            fs::create_dir_all(&records_root)?;
            for kind in ALL_KINDS {
                fs::create_dir_all(records_root.join(kind_dir(*kind)))?;
            }
        }

        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            clone_path,
            config: config.clone(),
            git: Arc::new(git),
        })
    }

    /// Workspace (code-repo) root.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Absolute path of the local data-repo clone.
    #[must_use]
    pub fn clone_path(&self) -> &Path {
        &self.clone_path
    }

    /// Active configuration.
    #[must_use]
    pub fn config(&self) -> &ExternalConfig {
        &self.config
    }

    /// Fetch the remote and fast-forward the configured default branch.
    ///
    /// Under the `loose` policy this is a strict fast-forward only: diverged
    /// histories return an error rather than implicitly producing a merge
    /// commit. Operators reconcile by hand (or by configuring `strict` /
    /// `auto-sync` in a future milestone).
    ///
    /// # Errors
    ///
    /// - [`StorageError::Git`] if the fetch or fast-forward fails.
    pub fn pull(&self) -> Result<(), StorageError> {
        run_git(
            &self.clone_path,
            &["fetch", "origin", &self.config.default_branch],
        )
        .map_err(StorageError::Git)?;
        // Fast-forward only.
        let ff = run_git(
            &self.clone_path,
            &[
                "merge",
                "--ff-only",
                &format!("origin/{}", self.config.default_branch),
            ],
        );
        match ff {
            Ok(_) => Ok(()),
            Err(e) => Err(StorageError::Git(e)),
        }
    }

    /// Push committed records to the remote default branch.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Git`] if the push fails (non-fast-forward, auth,
    ///   network).
    pub fn push(&self) -> Result<(), StorageError> {
        run_git(
            &self.clone_path,
            &["push", "origin", &self.config.default_branch],
        )
        .map_err(StorageError::Git)?;
        Ok(())
    }

    /// Build the path (relative to the clone root) backing `id`.
    fn relative_path(id: &RecordId) -> PathBuf {
        let lower = id.as_str().to_lowercase();
        PathBuf::from(RECORDS_DIR)
            .join(kind_dir(id.kind()))
            .join(format!("{lower}.json"))
    }

    /// Commit `path` (relative to the clone) into the local clone with a
    /// stable message.
    fn commit_record(&self, rel: &Path, id: &RecordId, kind: RecordOp) -> Result<(), StorageError> {
        let path_str = rel.to_string_lossy().to_string();
        run_git(&self.clone_path, &["add", "--", &path_str]).map_err(StorageError::Git)?;
        let msg = match kind {
            RecordOp::Write => format!("firetrail: write {}", id.as_str()),
            RecordOp::Delete => format!("firetrail: delete {}", id.as_str()),
        };
        // Skip empty commits (e.g. re-writing identical content) so callers
        // don't observe spurious "nothing to commit" errors as failures.
        match run_git(&self.clone_path, &["commit", "--quiet", "-m", &msg]) {
            Ok(_) => Ok(()),
            Err(ft_git::GitError::Shell { stderr, .. })
                if stderr.contains("nothing to commit")
                    || stderr.contains("nothing added to commit")
                    || stderr.contains("no changes added") =>
            {
                Ok(())
            }
            Err(e) => Err(StorageError::Git(e)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RecordOp {
    Write,
    Delete,
}

/// Every record kind, in stable order.
const ALL_KINDS: &[RecordKind] = &[
    RecordKind::Task,
    RecordKind::Epic,
    RecordKind::Subtask,
    RecordKind::Bug,
    RecordKind::Incident,
    RecordKind::Finding,
    RecordKind::Runbook,
    RecordKind::Decision,
    RecordKind::Gotcha,
    RecordKind::Memory,
    RecordKind::Doc,
    RecordKind::RepoProfile,
];

impl Storage for ExternalStorage {
    fn read(&self, id: &RecordId) -> Result<Record, StorageError> {
        let path = self.path_for(id);
        if !path.exists() {
            return Err(StorageError::NotFound(id.clone()));
        }
        let bytes = fs::read(&path)?;
        parse_and_validate(&path, &bytes)
    }

    fn read_at_ref(&self, gitref: &str, id: &RecordId) -> Result<Record, StorageError> {
        let rel = Self::relative_path(id);
        let bytes = match self.git.read_file_at_ref(gitref, &rel) {
            Ok(b) => b,
            Err(ft_git::GitError::FileNotInTree(_, _)) => {
                return Err(StorageError::NotFound(id.clone()));
            }
            Err(e) => return Err(StorageError::Git(e)),
        };
        let path = self.clone_path.join(&rel);
        parse_and_validate(&path, &bytes)
    }

    fn write(&self, record: &Record) -> Result<PathBuf, StorageError> {
        let recomputed = state_hash(record)?;
        if recomputed != record.envelope.state_hash {
            return Err(StorageError::HashMismatch {
                id: record.envelope.id.clone(),
                file_hash: record.envelope.state_hash.clone(),
                recomputed,
            });
        }

        let rel = Self::relative_path(&record.envelope.id);
        let path = self.clone_path.join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_vec_pretty(record)?;
        let tmp = tmp_path(&path);
        {
            let mut f = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        if let Some(parent) = path.parent() {
            sync_dir(parent)?;
        }

        // Auto-commit the write so the chain is preserved and `push` has
        // something to publish.
        self.commit_record(&rel, &record.envelope.id, RecordOp::Write)?;

        Ok(path)
    }

    fn delete(&self, id: &RecordId) -> Result<(), StorageError> {
        let rel = Self::relative_path(id);
        let path = self.clone_path.join(&rel);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound(id.clone()));
            }
            Err(e) => return Err(StorageError::Io(e)),
        }
        self.commit_record(&rel, id, RecordOp::Delete)?;
        Ok(())
    }

    fn list(&self, filter: &StorageFilter) -> Result<Vec<RecordId>, StorageError> {
        let mut out = Vec::new();
        for entry in iter_record_paths(&self.records_root(), filter.kinds.as_deref()) {
            let path = entry?;
            let bytes = fs::read(&path)?;
            let record = parse_and_validate(&path, &bytes)?;
            if filter.matches(&record) {
                out.push(record.envelope.id);
            }
        }
        Ok(out)
    }

    fn iter<'a>(
        &'a self,
        filter: &'a StorageFilter,
    ) -> Box<dyn Iterator<Item = Result<Record, StorageError>> + 'a> {
        let walker = iter_record_paths(&self.records_root(), filter.kinds.as_deref());
        Box::new(walker.filter_map(move |entry| match entry {
            Err(e) => Some(Err(e)),
            Ok(path) => match fs::read(&path).map_err(StorageError::Io) {
                Err(e) => Some(Err(e)),
                Ok(bytes) => match parse_and_validate(&path, &bytes) {
                    Err(e) => Some(Err(e)),
                    Ok(record) => {
                        if filter.matches(&record) {
                            Some(Ok(record))
                        } else {
                            None
                        }
                    }
                },
            },
        }))
    }

    fn path_for(&self, id: &RecordId) -> PathBuf {
        self.clone_path.join(Self::relative_path(id))
    }

    fn records_root(&self) -> PathBuf {
        self.clone_path.join(RECORDS_DIR)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Ensure a clone of `url` exists at `local_path`. Clones on first use;
/// fetches `origin` otherwise. Returns an opened [`Repo`].
///
/// # Errors
///
/// - [`StorageError::Io`] if the parent directory cannot be created.
/// - [`StorageError::Git`] if the clone or fetch shells fail, or if the
///   resulting directory is not a git repository.
pub fn ensure_data_repo_cloned(url: &str, local_path: &Path) -> Result<Repo, StorageError> {
    if local_path.join(".git").exists() {
        // Fetch updates; don't fail open() if remote is temporarily unreachable.
        let _ = run_git(local_path, &["fetch", "origin"]);
        return Repo::open(local_path).map_err(StorageError::Git);
    }
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Use shell git for clone; gix clone is not yet stable for our needs.
    let target = local_path.to_string_lossy().to_string();
    let output = Command::new("git")
        .args(["clone", "--quiet", url, &target])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(StorageError::Git(ft_git::GitError::Shell {
            exit: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }));
    }
    // Ensure a baseline identity is configured in the clone so auto-commits
    // succeed even when the operator's global config is absent (CI, tests).
    let _ = run_git(local_path, &["config", "user.email", "firetrail@local"]);
    let _ = run_git(local_path, &["config", "user.name", "Firetrail"]);
    let _ = run_git(local_path, &["config", "commit.gpgsign", "false"]);

    Repo::open(local_path).map_err(StorageError::Git)
}

/// Compute ahead/behind counts and dirty status for the storage's local
/// clone relative to its tracking branch.
///
/// # Errors
///
/// - [`StorageError::Git`] if any underlying git command fails.
pub fn sync_status(storage: &ExternalStorage) -> Result<SyncStatus, StorageError> {
    let branch = &storage.config.default_branch;
    let upstream = format!("origin/{branch}");

    // Refresh remote-tracking refs first; tolerate offline fetch failures so
    // `sync_status` is useful even without connectivity.
    let _ = run_git(&storage.clone_path, &["fetch", "origin", branch]);

    let counts_out = run_git(
        &storage.clone_path,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{upstream}...{branch}"),
        ],
    );
    let (behind, ahead) = match counts_out {
        Ok(out) => {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut parts = s.split_whitespace();
            let behind = parts
                .next()
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(0);
            let ahead = parts
                .next()
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(0);
            (behind, ahead)
        }
        // Upstream not configured yet (e.g. first push hasn't happened).
        Err(_) => (0, 0),
    };

    let status_out =
        run_git(&storage.clone_path, &["status", "--porcelain=v1"]).map_err(StorageError::Git)?;
    let dirty = !String::from_utf8_lossy(&status_out.stdout)
        .trim()
        .is_empty();

    Ok(SyncStatus {
        ahead,
        behind,
        dirty,
    })
}

/// Run a git subcommand inside `cwd`, capturing stdout/stderr. Maps non-zero
/// exit into [`ft_git::GitError::Shell`].
fn run_git(cwd: &Path, args: &[&str]) -> Result<Output, ft_git::GitError> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(ft_git::GitError::Io)?;
    if !output.status.success() {
        return Err(ft_git::GitError::Shell {
            exit: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output)
}

fn tmp_path(final_path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = format!(".{pid}.{tid}.{n}.tmp");
    let mut s = final_path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn iter_record_paths(
    records_root: &Path,
    kinds: Option<&[RecordKind]>,
) -> Box<dyn Iterator<Item = Result<PathBuf, StorageError>>> {
    let dirs: Vec<PathBuf> = match kinds {
        Some(kinds) => kinds
            .iter()
            .map(|k| records_root.join(kind_dir(*k)))
            .collect(),
        None => vec![records_root.to_path_buf()],
    };
    let mut entries: Vec<Result<PathBuf, StorageError>> = Vec::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir).min_depth(1).follow_links(false) {
            match entry {
                Err(e) => {
                    if let Some(io) = e.io_error() {
                        entries.push(Err(StorageError::Io(std::io::Error::new(
                            io.kind(),
                            io.to_string(),
                        ))));
                    } else {
                        entries.push(Err(StorageError::Io(std::io::Error::other(e.to_string()))));
                    }
                }
                Ok(e) => {
                    let path = e.path();
                    if e.file_type().is_file()
                        && path.extension().is_some_and(|x| x == "json")
                        && !path
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().ends_with(".tmp"))
                    {
                        entries.push(Ok(path.to_path_buf()));
                    }
                }
            }
        }
    }
    Box::new(entries.into_iter())
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> Result<(), StorageError> {
    let dir = File::open(path)?;
    dir.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

fn parse_and_validate(path: &Path, bytes: &[u8]) -> Result<Record, StorageError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| StorageError::Invalid {
            path: path.to_path_buf(),
            reason: format!("malformed json: {e}"),
        })?;
    if let Err(e) = validate_record_json(&value) {
        return Err(StorageError::Invalid {
            path: path.to_path_buf(),
            reason: format!("schema: {e}"),
        });
    }
    let record: Record = serde_json::from_value(value).map_err(|e| StorageError::Invalid {
        path: path.to_path_buf(),
        reason: format!("typed parse: {e}"),
    })?;
    let recomputed = state_hash(&record)?;
    if recomputed != record.envelope.state_hash {
        return Err(StorageError::HashMismatch {
            id: record.envelope.id.clone(),
            file_hash: record.envelope.state_hash.clone(),
            recomputed,
        });
    }
    Ok(record)
}
