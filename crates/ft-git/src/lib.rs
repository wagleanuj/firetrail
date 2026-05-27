//! # ft-git
//!
//! Git operations wrapper. Wraps `gix` for in-process reads and shells out to
//! the system `git` for writes that need full porcelain semantics.
//!
//! `ft-git` does not own a long-lived `gix::Repository` session — each call
//! opens a fresh handle. This avoids leaking lifetimes through the public API
//! and keeps the [`Repo`] struct trivially `Send`/`Sync`.
//!
//! ## Implementation choice: `gix` vs shelling out
//!
//! The spec is authoritative on the split. We use:
//!
//! - **`gix`** for [`Repo::head`], [`Repo::current_branch`], [`Repo::is_detached`],
//!   [`Repo::branches`], [`Repo::branch_exists`], [`Repo::read_file_at_ref`],
//!   [`Repo::list_files_at_ref`], [`Repo::log_path`], and [`Repo::diff`]. These
//!   are read-only and benefit from avoiding subprocess overhead.
//! - **`git` (shell-out)** for [`Repo::status`], [`Repo::is_clean`],
//!   [`Repo::has_uncommitted`], [`Repo::branch_create`], [`Repo::branch_delete`],
//!   and [`Repo::checkout`]. `gix` writes are not yet stable for our needs and
//!   working-tree status semantics from porcelain `git` are battle-tested.
//! - **Direct filesystem writes** for [`Repo::install_hook`],
//!   [`Repo::hook_installed`], and [`Repo::remove_hook`].
//!
//! ## Relevant ADRs
//!
//! - ADR-0002 — JSON-in-Git, not Dolt
//! - ADR-0017 — Audit-chain integrity (`pre-receive` protection)
//! - ADR-0018 — Branch salvage

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{DateTime, TimeZone, Utc};
use globset::{Glob, GlobMatcher};

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors produced by [`Repo`].
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The given path does not exist.
    #[error("repo not found at {0}")]
    NotFound(PathBuf),

    /// The path exists but does not contain a `.git` directory or file.
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),

    /// A ref could not be resolved.
    #[error("ref not found: {0}")]
    RefNotFound(String),

    /// A file was not present in the tree at the given ref.
    #[error("file not in tree: {0} at {1}")]
    FileNotInTree(PathBuf, String),

    /// A branch with that name already exists.
    #[error("branch already exists: {0}")]
    BranchExists(String),

    /// Hook install failed; typically a permissions or write error.
    #[error("hook install failed: {0}")]
    HookInstall(String),

    /// Underlying I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// `gix` reported an error.
    #[error("gix: {0}")]
    Gix(String),

    /// A `git` subprocess failed.
    #[error("git command failed (exit {exit}): {stderr}")]
    Shell {
        /// Exit code reported by the subprocess (-1 if killed by a signal).
        exit: i32,
        /// Captured stderr from the failed command.
        stderr: String,
    },
}

// ── Types ───────────────────────────────────────────────────────────────────

/// Information about a resolved ref (HEAD, a branch, or a tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefInfo {
    /// Full ref name, e.g. `refs/heads/main`, or `HEAD` if the symbolic ref
    /// could not be resolved to a branch (detached HEAD).
    pub name: String,
    /// Commit SHA the ref currently points at, hex-encoded.
    pub commit_sha: String,
    /// First line of the commit message.
    pub commit_summary: String,
}

/// Working-tree status snapshot, as produced by `git status --porcelain`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusReport {
    /// Whether the working tree has no uncommitted changes.
    pub clean: bool,
    /// Files modified but not staged.
    pub modified: Vec<PathBuf>,
    /// Files added or modified in the index.
    pub staged: Vec<PathBuf>,
    /// Files that exist on disk but are not tracked.
    pub untracked: Vec<PathBuf>,
    /// Files deleted from the working tree.
    pub deleted: Vec<PathBuf>,
}

/// Branch metadata returned by [`Repo::branches`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    /// Short branch name, e.g. `main`.
    pub name: String,
    /// Commit SHA the branch points at, hex-encoded.
    pub commit_sha: String,
    /// `true` for the currently checked-out branch.
    pub is_current: bool,
    /// Upstream tracking ref, if configured (e.g. `refs/remotes/origin/main`).
    pub upstream: Option<String>,
}

/// A single path-level change between two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    /// Path of the changed file in the new tree (or the old tree for deletes).
    pub path: PathBuf,
    /// Nature of the change.
    pub change_kind: ChangeKind,
    /// Blob SHA in the old tree, if any.
    pub old_sha: Option<String>,
    /// Blob SHA in the new tree, if any.
    pub new_sha: Option<String>,
}

/// Kind of change in a [`DiffEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// File present in the new tree but not the old.
    Added,
    /// File present in both trees with different content.
    Modified,
    /// File present in the old tree but not the new.
    Deleted,
    /// File renamed from the recorded old path.
    Renamed {
        /// Path the file was renamed from in the old tree.
        from: PathBuf,
    },
}

/// Commit metadata returned by [`Repo::log_path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// Commit SHA, hex-encoded.
    pub sha: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub author_email: String,
    /// Author timestamp as UTC.
    pub time: DateTime<Utc>,
    /// First line of the commit message.
    pub summary: String,
}

/// Identifies a git hook by its on-disk file name (or by its server-side
/// artifact destination for [`HookName::PreReceiveProtectFiretrail`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookName {
    /// `pre-commit`.
    PreCommit,
    /// `post-checkout`.
    PostCheckout,
    /// `post-merge`.
    PostMerge,
    /// `post-commit`.
    PostCommit,
    /// `pre-receive`.
    PreReceive,
    /// Server-side hook emitted as an artifact under `.firetrail/hooks/` rather
    /// than installed into `.git/hooks/`. See ADR-0017.
    PreReceiveProtectFiretrail,
}

impl HookName {
    /// The on-disk file name for this hook under `.git/hooks/`.
    #[must_use]
    pub fn filename(self) -> &'static str {
        match self {
            HookName::PreCommit => "pre-commit",
            HookName::PostCheckout => "post-checkout",
            HookName::PostMerge => "post-merge",
            HookName::PostCommit => "post-commit",
            HookName::PreReceive | HookName::PreReceiveProtectFiretrail => "pre-receive",
        }
    }
}

/// Marker for the managed region of a git hook script.
const MARK_BEGIN: &str = "# >>> firetrail managed >>>";
/// Closing marker for the managed region of a git hook script.
const MARK_END: &str = "# <<< firetrail managed <<<";

// ── Repo ────────────────────────────────────────────────────────────────────

/// Handle to a git repository.
///
/// Cheap to clone — only holds the canonical path of the work-tree root. All
/// methods open a fresh `gix::Repository` (or shell out) on each call.
#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Open the repo at the given path. Verifies a `.git` directory or file
    /// exists at the path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert_eq!(repo.root(), tr.root());
    /// ```
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GitError> {
        let root = root.into();
        if !root.exists() {
            return Err(GitError::NotFound(root));
        }
        let dot_git = root.join(".git");
        if !dot_git.exists() {
            return Err(GitError::NotARepo(root));
        }
        Ok(Self { root })
    }

    /// Discover the repo by walking up from the given starting path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let nested = tr.root().join(".firetrail/records/task");
    /// let repo = Repo::discover(&nested).unwrap();
    /// assert_eq!(repo.root(), tr.root());
    /// ```
    pub fn discover(from: impl AsRef<Path>) -> Result<Self, GitError> {
        let from = from.as_ref();
        let r = gix::ThreadSafeRepository::discover(from)
            .map_err(|e| GitError::Gix(e.to_string()))?
            .to_thread_local();
        let work = r
            .work_dir()
            .ok_or_else(|| GitError::NotARepo(from.to_path_buf()))?
            .to_path_buf();
        Ok(Self { root: work })
    }

    /// Absolute path of the work-tree root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ── Refs and status ────────────────────────────────────────────────────

    /// Resolve HEAD and return ref name, commit SHA, and commit summary.
    ///
    /// On detached HEAD, the `name` field is `"HEAD"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let head = repo.head().unwrap();
    /// assert_eq!(head.commit_summary, "initial");
    /// ```
    pub fn head(&self) -> Result<RefInfo, GitError> {
        let r = self.gix()?;
        let mut head = r.head().map_err(g)?;
        // Capture the ref name before mutating-borrowing head for the peel.
        let name = head
            .referent_name()
            .map_or_else(|| "HEAD".to_string(), |n| n.as_bstr().to_string());
        let commit = head.peel_to_commit_in_place().map_err(g)?;
        let id = commit.id;
        let summary = commit.message().map_err(g)?.summary().to_string();
        Ok(RefInfo {
            name,
            commit_sha: id.to_hex().to_string(),
            commit_summary: summary,
        })
    }

    /// Return the current branch name, or `None` on detached HEAD.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert_eq!(repo.current_branch().unwrap().as_deref(), Some("main"));
    /// ```
    pub fn current_branch(&self) -> Result<Option<String>, GitError> {
        let r = self.gix()?;
        let head = r.head().map_err(g)?;
        match head.referent_name() {
            Some(name) => {
                let full = name.as_bstr().to_string();
                Ok(Some(
                    full.strip_prefix("refs/heads/")
                        .map(str::to_string)
                        .unwrap_or(full),
                ))
            }
            None => Ok(None),
        }
    }

    /// Whether HEAD is detached (does not point at a branch).
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert!(!repo.is_detached().unwrap());
    /// ```
    pub fn is_detached(&self) -> Result<bool, GitError> {
        Ok(self.current_branch()?.is_none())
    }

    /// Whether the working tree has no uncommitted changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert!(repo.is_clean().unwrap());
    /// ```
    pub fn is_clean(&self) -> Result<bool, GitError> {
        let out = self.run_git(&["status", "--porcelain=v1"])?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().is_empty())
    }

    /// Whether the given path (relative to the repo root) has uncommitted
    /// changes (staged, unstaged, or untracked).
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert!(!repo.has_uncommitted("README.md").unwrap());
    /// ```
    pub fn has_uncommitted(&self, path: impl AsRef<Path>) -> Result<bool, GitError> {
        let p = path.as_ref();
        let p_str = p.to_string_lossy().to_string();
        let out = self.run_git(&["status", "--porcelain=v1", "--", &p_str])?;
        Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
    }

    /// Snapshot of the working tree status.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let s = repo.status().unwrap();
    /// assert!(s.clean);
    /// ```
    pub fn status(&self) -> Result<StatusReport, GitError> {
        let out = self.run_git(&["status", "--porcelain=v1", "-z"])?;
        let bytes = &out.stdout;
        let mut report = StatusReport {
            clean: bytes.is_empty(),
            ..StatusReport::default()
        };

        let mut i = 0;
        while i < bytes.len() {
            // Each entry: "XY <path>\0" optionally followed by "<orig>\0" for renames.
            if i + 3 >= bytes.len() {
                break;
            }
            let x = bytes[i];
            let y = bytes[i + 1];
            // bytes[i + 2] is ' '
            i += 3;
            // Read path until NUL.
            let start = i;
            while i < bytes.len() && bytes[i] != 0 {
                i += 1;
            }
            let path = PathBuf::from(String::from_utf8_lossy(&bytes[start..i]).into_owned());
            i += 1; // skip NUL

            // For renames/copies, the original path follows.
            if x == b'R' || x == b'C' {
                let s = i;
                while i < bytes.len() && bytes[i] != 0 {
                    i += 1;
                }
                i += 1;
                // For status purposes, treat the new path as staged.
                let _orig = &bytes[s..i.saturating_sub(1)];
            }

            match (x, y) {
                (b'?', b'?') => report.untracked.push(path),
                (b' ', b'M') => report.modified.push(path),
                (b' ', b'D') => report.deleted.push(path),
                (b'D', _) => {
                    report.staged.push(path.clone());
                    report.deleted.push(path);
                }
                (b'M', b' ' | b'M') | (b'A' | b'R' | b'C', _) => {
                    report.staged.push(path);
                }
                _ => {
                    // Fallback: classify as modified.
                    if y == b'M' {
                        report.modified.push(path);
                    } else {
                        report.staged.push(path);
                    }
                }
            }
        }

        Ok(report)
    }

    // ── Branches ───────────────────────────────────────────────────────────

    /// List all local branches.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let bs = repo.branches().unwrap();
    /// assert!(bs.iter().any(|b| b.name == "main" && b.is_current));
    /// ```
    pub fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
        let r = self.gix()?;
        let current = self.current_branch()?;
        let platform = r.references().map_err(g)?;
        let prefixed = platform.prefixed("refs/heads/").map_err(g)?;

        let mut out = Vec::new();
        for reference in prefixed {
            let mut reference = reference.map_err(g)?;
            let full = reference.name().as_bstr().to_string();
            let short = full
                .strip_prefix("refs/heads/")
                .map_or_else(|| full.clone(), str::to_string);

            // Peel to commit.
            let id = reference.peel_to_id_in_place().map_err(g)?;
            let sha = id.to_hex().to_string();

            // Upstream lookup via config: branch.<name>.merge + branch.<name>.remote.
            let upstream = upstream_for(&r, &short);

            out.push(BranchInfo {
                name: short.clone(),
                commit_sha: sha,
                is_current: current.as_deref() == Some(short.as_str()),
                upstream,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Whether a local branch with the given name exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert!(repo.branch_exists("main").unwrap());
    /// assert!(!repo.branch_exists("nope").unwrap());
    /// ```
    pub fn branch_exists(&self, name: &str) -> Result<bool, GitError> {
        let r = self.gix()?;
        let ref_name = format!("refs/heads/{name}");
        match r.try_find_reference(ref_name.as_str()).map_err(g)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Create a new branch pointing at the commit `from` resolves to.
    ///
    /// Returns [`GitError::BranchExists`] if `name` already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// repo.branch_create("feat/x", "main").unwrap();
    /// assert!(repo.branch_exists("feat/x").unwrap());
    /// ```
    pub fn branch_create(&self, name: &str, from: &str) -> Result<(), GitError> {
        if self.branch_exists(name)? {
            return Err(GitError::BranchExists(name.to_string()));
        }
        self.run_git(&["branch", name, from])?;
        Ok(())
    }

    /// Delete a local branch.
    ///
    /// If `force` is `true` the branch is deleted even if it is not fully
    /// merged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// repo.branch_create("tmp", "main").unwrap();
    /// repo.branch_delete("tmp", true).unwrap();
    /// assert!(!repo.branch_exists("tmp").unwrap());
    /// ```
    pub fn branch_delete(&self, name: &str, force: bool) -> Result<(), GitError> {
        let flag = if force { "-D" } else { "-d" };
        self.run_git(&["branch", flag, name])?;
        Ok(())
    }

    /// Check out an existing branch.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// repo.branch_create("feat/y", "main").unwrap();
    /// repo.checkout("feat/y").unwrap();
    /// assert_eq!(repo.current_branch().unwrap().as_deref(), Some("feat/y"));
    /// ```
    pub fn checkout(&self, name: &str) -> Result<(), GitError> {
        self.run_git(&["checkout", "--quiet", name])?;
        Ok(())
    }

    // ── File reads at a given ref ──────────────────────────────────────────

    /// Read the raw bytes of `path` as it exists at `gitref`.
    ///
    /// Returns [`GitError::FileNotInTree`] if the path does not exist at the
    /// ref.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// std::fs::write(tr.root().join("hello.txt"), b"hi\n").unwrap();
    /// tr.commit_all("add hello").unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let bytes = repo.read_file_at_ref("HEAD", "hello.txt").unwrap();
    /// assert_eq!(bytes, b"hi\n");
    /// ```
    pub fn read_file_at_ref(
        &self,
        gitref: &str,
        path: impl AsRef<Path>,
    ) -> Result<Vec<u8>, GitError> {
        let path = path.as_ref().to_path_buf();
        let r = self.gix()?;
        let tree = Self::resolve_to_tree(&r, gitref)?;
        let normalized = normalize_path(&path);
        let mut buf = Vec::new();
        let entry = tree
            .lookup_entry_by_path(&normalized, &mut buf)
            .map_err(g)?
            .ok_or_else(|| GitError::FileNotInTree(path.clone(), gitref.to_string()))?;
        let object = r.find_object(entry.object_id()).map_err(g)?;
        let blob = object.try_into_blob().map_err(g)?;
        Ok(blob.data.clone())
    }

    /// List files at `gitref` whose path matches `glob`.
    ///
    /// The glob is matched against the full repository-relative path. Paths
    /// are returned sorted lexicographically.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// std::fs::create_dir_all(tr.root().join(".firetrail/records/task")).unwrap();
    /// std::fs::write(tr.root().join(".firetrail/records/task/a.json"), b"{}").unwrap();
    /// tr.commit_all("seed").unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let files = repo
    ///     .list_files_at_ref("HEAD", ".firetrail/records/**/*.json")
    ///     .unwrap();
    /// assert_eq!(
    ///     files,
    ///     vec![std::path::PathBuf::from(".firetrail/records/task/a.json")]
    /// );
    /// ```
    pub fn list_files_at_ref(&self, gitref: &str, glob: &str) -> Result<Vec<PathBuf>, GitError> {
        let matcher = compile_glob(glob)?;
        let r = self.gix()?;
        let tree = Self::resolve_to_tree(&r, gitref)?;

        let mut out = Vec::new();
        walk_tree(&r, &tree, Path::new(""), &matcher, &mut out)?;
        out.sort();
        Ok(out)
    }

    // ── Diff and log ───────────────────────────────────────────────────────

    /// Compute a tree-to-tree diff between two refs, optionally filtered to
    /// paths starting with `path_filter`.
    ///
    /// Rename detection is performed by `gix`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// std::fs::write(tr.root().join("a.txt"), b"v1").unwrap();
    /// tr.commit_all("add a").unwrap();
    /// std::fs::write(tr.root().join("a.txt"), b"v2").unwrap();
    /// tr.commit_all("modify a").unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let entries = repo.diff("HEAD~1", "HEAD", None).unwrap();
    /// assert_eq!(entries.len(), 1);
    /// ```
    pub fn diff(
        &self,
        from: &str,
        to: &str,
        path_filter: Option<&str>,
    ) -> Result<Vec<DiffEntry>, GitError> {
        let r = self.gix()?;
        let from_tree = Self::resolve_to_tree(&r, from)?;
        let to_tree = Self::resolve_to_tree(&r, to)?;

        let mut entries: Vec<DiffEntry> = Vec::new();
        let mut platform = from_tree.changes().map_err(g)?;
        platform.track_path();
        platform
            .for_each_to_obtain_tree(
                &to_tree,
                |change| -> Result<gix::object::tree::diff::Action, std::convert::Infallible> {
                    use gix::object::tree::diff::change::Event;
                    let location = change.location.to_string();
                    match change.event {
                        Event::Addition { id, .. } => entries.push(DiffEntry {
                            path: PathBuf::from(location),
                            change_kind: ChangeKind::Added,
                            old_sha: None,
                            new_sha: Some(id.to_hex().to_string()),
                        }),
                        Event::Deletion { id, .. } => entries.push(DiffEntry {
                            path: PathBuf::from(location),
                            change_kind: ChangeKind::Deleted,
                            old_sha: Some(id.to_hex().to_string()),
                            new_sha: None,
                        }),
                        Event::Modification {
                            previous_id, id, ..
                        } => entries.push(DiffEntry {
                            path: PathBuf::from(location),
                            change_kind: ChangeKind::Modified,
                            old_sha: Some(previous_id.to_hex().to_string()),
                            new_sha: Some(id.to_hex().to_string()),
                        }),
                        Event::Rewrite {
                            source_location,
                            source_id,
                            id,
                            ..
                        } => entries.push(DiffEntry {
                            path: PathBuf::from(location),
                            change_kind: ChangeKind::Renamed {
                                from: PathBuf::from(source_location.to_string()),
                            },
                            old_sha: Some(source_id.to_hex().to_string()),
                            new_sha: Some(id.to_hex().to_string()),
                        }),
                    }
                    Ok(gix::object::tree::diff::Action::Continue)
                },
            )
            .map_err(g)?;

        if let Some(filter) = path_filter {
            entries.retain(|e| {
                e.path.to_string_lossy().starts_with(filter)
                    || match &e.change_kind {
                        ChangeKind::Renamed { from } => from.to_string_lossy().starts_with(filter),
                        _ => false,
                    }
            });
        }

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// Return commits that touched `path`, newest first. `limit` caps the
    /// number returned (`None` returns all).
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// std::fs::write(tr.root().join("log.txt"), b"v1").unwrap();
    /// tr.commit_all("first").unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// let commits = repo.log_path("log.txt", Some(10)).unwrap();
    /// assert_eq!(commits.len(), 1);
    /// assert_eq!(commits[0].summary, "first");
    /// ```
    pub fn log_path(
        &self,
        path: impl AsRef<Path>,
        limit: Option<usize>,
    ) -> Result<Vec<CommitInfo>, GitError> {
        let path = normalize_path(path.as_ref());
        let r = self.gix()?;
        let head_id = r.head_id().map_err(g)?;

        let walk = r.rev_walk([head_id]).all().map_err(g)?;

        let mut out = Vec::new();

        // Walk newest-first. Emit a commit when the path's blob differs from
        // the path's blob in any of the commit's first parents (or when the
        // path appears in the commit but did not exist in any parent). This
        // mirrors what `git log -- <path>` shows in linear histories.
        for info in walk {
            let info = info.map_err(g)?;
            let commit = r
                .find_object(info.id)
                .map_err(g)?
                .try_into_commit()
                .map_err(g)?;
            let tree = commit.tree().map_err(g)?;
            let mut buf = Vec::new();
            let entry = tree.lookup_entry_by_path(&path, &mut buf).map_err(g)?;
            let Some(entry) = entry else { continue };
            let blob_id = entry.object_id();

            let parent_ids: Vec<_> = commit.parent_ids().collect();
            let parents_blobs: Vec<Option<gix::ObjectId>> = if parent_ids.is_empty() {
                vec![None]
            } else {
                let mut v = Vec::with_capacity(parent_ids.len());
                for pid in parent_ids {
                    let parent_commit = r
                        .find_object(pid)
                        .map_err(g)?
                        .try_into_commit()
                        .map_err(g)?;
                    let parent_tree = parent_commit.tree().map_err(g)?;
                    let mut pbuf = Vec::new();
                    let pentry = parent_tree
                        .lookup_entry_by_path(&path, &mut pbuf)
                        .map_err(g)?;
                    v.push(pentry.map(|e| e.object_id()));
                }
                v
            };

            // Emit if the blob differs from every parent's blob.
            let differs_from_all_parents =
                parents_blobs.iter().all(|pb| pb.as_ref() != Some(&blob_id));

            if differs_from_all_parents {
                let msg = commit.message().map_err(g)?;
                let author = commit.author().map_err(g)?;
                let ts = author.time;
                out.push(CommitInfo {
                    sha: info.id.to_hex().to_string(),
                    author: author.name.to_string(),
                    author_email: author.email.to_string(),
                    time: Utc
                        .timestamp_opt(ts.seconds, 0)
                        .single()
                        .unwrap_or_else(Utc::now),
                    summary: msg.summary().to_string(),
                });
                if let Some(cap) = limit {
                    if out.len() >= cap {
                        break;
                    }
                }
            }
        }

        Ok(out)
    }

    // ── Hooks ──────────────────────────────────────────────────────────────

    /// Install or update the managed section of a git hook.
    ///
    /// For [`HookName::PreReceiveProtectFiretrail`] the script is written to
    /// `.firetrail/hooks/pre-receive` (not into `.git/hooks/`).
    ///
    /// For all other variants, the hook file under `.git/hooks/<name>` is
    /// created with mode `0o755`. If the file already exists, only the
    /// managed region (delimited by markers) is replaced; content outside the
    /// markers is preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::{HookName, Repo};
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// repo.install_hook(HookName::PreCommit, "echo hi").unwrap();
    /// assert!(repo.hook_installed(HookName::PreCommit));
    /// ```
    pub fn install_hook(&self, name: HookName, content: &str) -> Result<(), GitError> {
        if matches!(name, HookName::PreReceiveProtectFiretrail) {
            let dir = self.root.join(".firetrail/hooks");
            std::fs::create_dir_all(&dir)
                .map_err(|e| GitError::HookInstall(format!("create {}: {e}", dir.display())))?;
            let path = dir.join(name.filename());
            write_managed(&path, content)?;
            set_executable(&path)?;
            return Ok(());
        }

        let dir = self.root.join(".git/hooks");
        std::fs::create_dir_all(&dir)
            .map_err(|e| GitError::HookInstall(format!("create {}: {e}", dir.display())))?;
        let path = dir.join(name.filename());
        write_managed(&path, content)?;
        set_executable(&path)?;
        Ok(())
    }

    /// Whether the hook file exists on disk.
    ///
    /// For [`HookName::PreReceiveProtectFiretrail`] looks under
    /// `.firetrail/hooks/`; for all others under `.git/hooks/`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::{HookName, Repo};
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// assert!(!repo.hook_installed(HookName::PreCommit));
    /// ```
    #[must_use]
    pub fn hook_installed(&self, name: HookName) -> bool {
        self.hook_path(name).exists()
    }

    /// Remove the hook file (or the managed region from a hook that contains
    /// user content outside the markers).
    ///
    /// If the file does not exist, returns `Ok(())`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::{HookName, Repo};
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// repo.install_hook(HookName::PreCommit, "echo hi").unwrap();
    /// repo.remove_hook(HookName::PreCommit).unwrap();
    /// assert!(!repo.hook_installed(HookName::PreCommit));
    /// ```
    pub fn remove_hook(&self, name: HookName) -> Result<(), GitError> {
        let path = self.hook_path(name);
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| GitError::HookInstall(format!("read {}: {e}", path.display())))?;
        let stripped = strip_managed(&content);
        let user_text = stripped.trim();
        let only_shebang_or_blank = user_text.is_empty()
            || user_text
                .lines()
                .all(|l| l.trim().is_empty() || l.trim_start().starts_with("#!"));
        if only_shebang_or_blank {
            std::fs::remove_file(&path)
                .map_err(|e| GitError::HookInstall(format!("remove {}: {e}", path.display())))?;
        } else {
            std::fs::write(&path, stripped)
                .map_err(|e| GitError::HookInstall(format!("write {}: {e}", path.display())))?;
        }
        Ok(())
    }

    // ── Config ─────────────────────────────────────────────────────────────

    /// Look up a single `git config` value by dotted key (e.g. `user.email`).
    ///
    /// Reads the merged config snapshot via `gix`, honoring the normal scope
    /// chain (system, global / user, local) and the `GIT_CONFIG_GLOBAL` /
    /// `GIT_CONFIG_SYSTEM` / `GIT_CONFIG_NOSYSTEM` environment variables that
    /// gix consults through `gix-config`.
    ///
    /// Returns `Ok(None)` when the key is unset OR when the resolved value is
    /// empty / whitespace-only. The returned string is trimmed.
    ///
    /// # Errors
    ///
    /// Propagates [`GitError::Gix`] when opening the repository or its config
    /// snapshot fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ft_git::Repo;
    /// use ft_testkit::TestRepo;
    ///
    /// let tr = TestRepo::new().unwrap();
    /// let repo = Repo::open(tr.root()).unwrap();
    /// // TestRepo sets user.email via `git config`.
    /// assert!(repo.config_value("user.email").unwrap().is_some());
    /// assert!(repo.config_value("does.not.exist").unwrap().is_none());
    /// ```
    pub fn config_value(&self, key: &str) -> Result<Option<String>, GitError> {
        let r = self.gix()?;
        let snapshot = r.config_snapshot();
        let raw = snapshot.string(key);
        Ok(raw.and_then(|c| {
            let s = c.to_string();
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }))
    }

    // ── Internals ──────────────────────────────────────────────────────────

    fn hook_path(&self, name: HookName) -> PathBuf {
        if matches!(name, HookName::PreReceiveProtectFiretrail) {
            self.root.join(".firetrail/hooks").join(name.filename())
        } else {
            self.root.join(".git/hooks").join(name.filename())
        }
    }

    fn gix(&self) -> Result<gix::Repository, GitError> {
        gix::open(&self.root).map_err(|e| GitError::Gix(e.to_string()))
    }

    fn resolve_to_tree<'r>(
        r: &'r gix::Repository,
        gitref: &str,
    ) -> Result<gix::Tree<'r>, GitError> {
        let spec = r
            .rev_parse(gitref)
            .map_err(|_| GitError::RefNotFound(gitref.to_string()))?;
        let id = spec
            .single()
            .ok_or_else(|| GitError::RefNotFound(gitref.to_string()))?;
        let object = r
            .find_object(id)
            .map_err(|_| GitError::RefNotFound(gitref.to_string()))?;
        let commit = object
            .peel_to_kind(gix::object::Kind::Commit)
            .map_err(|_| GitError::RefNotFound(gitref.to_string()))?
            .into_commit();
        commit.tree().map_err(g)
    }

    /// Run a git subcommand in the repo root, capturing stdout/stderr.
    fn run_git(&self, args: &[&str]) -> Result<Output, GitError> {
        let output = Command::new("git")
            .arg("--no-pager")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;
        if !output.status.success() {
            return Err(GitError::Shell {
                exit: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(output)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Map any `Display` error from `gix` into [`GitError::Gix`].
fn g<E: std::fmt::Display>(e: E) -> GitError {
    GitError::Gix(e.to_string())
}

/// Normalize a path: forward slashes, no leading `./`, no trailing slash.
fn normalize_path(p: &Path) -> PathBuf {
    let mut s = p.to_string_lossy().replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    if s.ends_with('/') {
        s.pop();
    }
    PathBuf::from(s)
}

/// Compile a glob pattern.
fn compile_glob(pat: &str) -> Result<GlobMatcher, GitError> {
    Glob::new(pat)
        .map(|g| g.compile_matcher())
        .map_err(|e| GitError::Gix(format!("invalid glob '{pat}': {e}")))
}

/// Recursively walk a tree, collecting paths that match `matcher`.
fn walk_tree(
    r: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: &Path,
    matcher: &GlobMatcher,
    out: &mut Vec<PathBuf>,
) -> Result<(), GitError> {
    // Collect entries into owned data first to avoid borrow issues across recursion.
    let snapshot: Vec<(String, gix::ObjectId, gix::object::tree::EntryKind)> = tree
        .iter()
        .map(|e| {
            let e = e.map_err(g)?;
            Ok::<_, GitError>((e.filename().to_string(), e.object_id(), e.mode().kind()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    for (name, oid, kind) in snapshot {
        let path = prefix.join(&name);
        match kind {
            gix::object::tree::EntryKind::Tree => {
                let sub_tree = r.find_object(oid).map_err(g)?.into_tree();
                walk_tree(r, &sub_tree, &path, matcher, out)?;
            }
            gix::object::tree::EntryKind::Blob | gix::object::tree::EntryKind::BlobExecutable => {
                if matcher.is_match(&path) {
                    out.push(path);
                }
            }
            _ => {
                // Symlinks, commits (submodules): out of scope.
            }
        }
    }
    Ok(())
}

/// Look up the configured upstream for a local branch via
/// `branch.<name>.remote` + `branch.<name>.merge`.
fn upstream_for(r: &gix::Repository, branch: &str) -> Option<String> {
    let cfg = r.config_snapshot();
    let plumb = cfg.plumbing();
    let remote = plumb
        .string_by("branch", Some(branch.into()), "remote")
        .map(|c| c.to_string())?;
    let merge = plumb
        .string_by("branch", Some(branch.into()), "merge")
        .map(|c| c.to_string())?;
    // Convert refs/heads/foo on remote "origin" to refs/remotes/origin/foo.
    let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    Some(format!("refs/remotes/{remote}/{short}"))
}

/// Write `content` into a hook script at `path`, preserving any pre-existing
/// content outside the firetrail-managed markers.
fn write_managed(path: &Path, content: &str) -> Result<(), GitError> {
    let existing = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| GitError::HookInstall(format!("read {}: {e}", path.display())))?
    } else {
        String::new()
    };

    let managed_block = format!("{MARK_BEGIN}\n{content}\n{MARK_END}\n");

    let new_content = if existing.is_empty() {
        format!("#!/bin/sh\n{managed_block}")
    } else if existing.contains(MARK_BEGIN) && existing.contains(MARK_END) {
        // Replace the marked region in place.
        let before = existing.split_once(MARK_BEGIN).map_or("", |(b, _)| b);
        let after = existing.split_once(MARK_END).map_or("", |(_, a)| a);
        let after = after.strip_prefix('\n').unwrap_or(after);
        format!("{before}{managed_block}{after}")
    } else {
        // Append managed block.
        let sep = if existing.ends_with('\n') { "" } else { "\n" };
        format!("{existing}{sep}{managed_block}")
    };

    std::fs::write(path, new_content)
        .map_err(|e| GitError::HookInstall(format!("write {}: {e}", path.display())))?;
    Ok(())
}

/// Remove the firetrail-managed block from a hook script, if present.
fn strip_managed(content: &str) -> String {
    if !content.contains(MARK_BEGIN) {
        return content.to_string();
    }
    let before = content.split_once(MARK_BEGIN).map_or("", |(b, _)| b);
    let after = content.split_once(MARK_END).map_or("", |(_, a)| a);
    let after = after.strip_prefix('\n').unwrap_or(after);
    format!("{}{}", before.trim_end_matches('\n'), {
        if after.is_empty() {
            String::new()
        } else {
            format!("\n{after}")
        }
    })
}

/// Set the file at `path` executable (mode `0o755`).
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), GitError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| GitError::HookInstall(format!("stat {}: {e}", path.display())))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .map_err(|e| GitError::HookInstall(format!("chmod {}: {e}", path.display())))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), GitError> {
    Ok(())
}

// ── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_strips_dot_slash() {
        assert_eq!(
            normalize_path(Path::new("./a/b.txt")),
            PathBuf::from("a/b.txt")
        );
        assert_eq!(normalize_path(Path::new("a/b/")), PathBuf::from("a/b"));
    }

    #[test]
    fn strip_managed_removes_marked_region() {
        let text =
            format!("#!/bin/sh\necho before\n{MARK_BEGIN}\necho managed\n{MARK_END}\necho after\n");
        let stripped = strip_managed(&text);
        assert!(!stripped.contains("echo managed"));
        assert!(stripped.contains("echo before"));
        assert!(stripped.contains("echo after"));
    }

    #[test]
    fn hookname_filenames_are_stable() {
        assert_eq!(HookName::PreCommit.filename(), "pre-commit");
        assert_eq!(HookName::PostCheckout.filename(), "post-checkout");
        assert_eq!(HookName::PostMerge.filename(), "post-merge");
        assert_eq!(HookName::PostCommit.filename(), "post-commit");
        assert_eq!(HookName::PreReceive.filename(), "pre-receive");
        assert_eq!(
            HookName::PreReceiveProtectFiretrail.filename(),
            "pre-receive"
        );
    }
}
