# ft-git — git operations wrapper

**Epic:** `firetrail-vth`
**Wave:** 1
**Depends on:** workspace skeleton
**Depended on by:** ft-storage, ft-cli, ft-history (M2), ft-pr (M4)

---

## Purpose

`ft-git` wraps the git operations Firetrail needs behind a clean Rust API. Two
goals: keep git knowledge out of higher layers, and let us swap implementations
(currently `gix` for reads, shell-out to `git` for some writes) without touching
callers.

`ft-git` does not own a long-lived `Repo` handle that maps to a `gix::Repository`
session — each call opens cheaply. This avoids leaking implementation details
through lifetimes.

---

## Public API

### Repo

```rust
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Open the repo at the given path. Verifies a .git directory or file exists.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GitError>;

    /// Discover the repo by walking up from the current directory.
    pub fn discover(from: impl AsRef<Path>) -> Result<Self, GitError>;

    pub fn root(&self) -> &Path;

    // ── Refs and status ──────────────────────────────────────────────────────
    pub fn head(&self) -> Result<RefInfo, GitError>;
    pub fn current_branch(&self) -> Result<Option<String>, GitError>;
    pub fn is_detached(&self) -> Result<bool, GitError>;
    pub fn is_clean(&self) -> Result<bool, GitError>;
    pub fn has_uncommitted(&self, path: impl AsRef<Path>) -> Result<bool, GitError>;
    pub fn status(&self) -> Result<StatusReport, GitError>;

    // ── Branches ─────────────────────────────────────────────────────────────
    pub fn branches(&self) -> Result<Vec<BranchInfo>, GitError>;
    pub fn branch_exists(&self, name: &str) -> Result<bool, GitError>;
    pub fn branch_create(&self, name: &str, from: &str) -> Result<(), GitError>;
    pub fn branch_delete(&self, name: &str, force: bool) -> Result<(), GitError>;
    pub fn checkout(&self, name: &str) -> Result<(), GitError>;

    // ── File reads at a given ref ────────────────────────────────────────────
    pub fn read_file_at_ref(&self, gitref: &str, path: impl AsRef<Path>)
        -> Result<Vec<u8>, GitError>;

    pub fn list_files_at_ref(&self, gitref: &str, glob: &str)
        -> Result<Vec<PathBuf>, GitError>;

    // ── Diff and log ─────────────────────────────────────────────────────────
    pub fn diff(&self, from: &str, to: &str, path_filter: Option<&str>)
        -> Result<Vec<DiffEntry>, GitError>;

    pub fn log_path(&self, path: impl AsRef<Path>, limit: Option<usize>)
        -> Result<Vec<CommitInfo>, GitError>;

    // ── Hooks ────────────────────────────────────────────────────────────────
    pub fn install_hook(&self, name: HookName, content: &str)
        -> Result<(), GitError>;
    pub fn hook_installed(&self, name: HookName) -> bool;
    pub fn remove_hook(&self, name: HookName) -> Result<(), GitError>;
}
```

### Types

```rust
pub struct RefInfo {
    pub name: String,           // e.g. "refs/heads/main" or "HEAD"
    pub commit_sha: String,
    pub commit_summary: String,
}

pub struct StatusReport {
    pub clean: bool,
    pub modified: Vec<PathBuf>,
    pub staged: Vec<PathBuf>,
    pub untracked: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
}

pub struct BranchInfo {
    pub name: String,
    pub commit_sha: String,
    pub is_current: bool,
    pub upstream: Option<String>,
}

pub struct DiffEntry {
    pub path: PathBuf,
    pub change_kind: ChangeKind,
    pub old_sha: Option<String>,
    pub new_sha: Option<String>,
}

pub enum ChangeKind { Added, Modified, Deleted, Renamed { from: PathBuf } }

pub struct CommitInfo {
    pub sha: String,
    pub author: String,
    pub author_email: String,
    pub time: DateTime<Utc>,
    pub summary: String,
}

pub enum HookName {
    PreCommit,
    PostCheckout,
    PostMerge,
    PostCommit,
    PreReceive,
    PreReceiveProtectFiretrail,  // server-side; emitted as artifact, not installed
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("repo not found at {0}")]
    NotFound(PathBuf),

    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),

    #[error("ref not found: {0}")]
    RefNotFound(String),

    #[error("file not in tree: {0} at {1}")]
    FileNotInTree(PathBuf, String),

    #[error("branch already exists: {0}")]
    BranchExists(String),

    #[error("hook install failed: {0}")]
    HookInstall(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("gix: {0}")]
    Gix(String),

    #[error("git command failed (exit {exit}): {stderr}")]
    Shell { exit: i32, stderr: String },
}
```

---

## Internal design

### Read-heavy paths use `gix`

`read_file_at_ref`, `list_files_at_ref`, `head`, `current_branch`, `is_detached`,
`branches`, `branch_exists`, `log_path`, `diff` — all implemented via `gix`. No
subprocess overhead.

### Write paths shell out

`branch_create`, `branch_delete`, `checkout`, `install_hook` (uses filesystem
writes, not `gix`), `commit` operations called from ft-storage — shell out to the
system `git`. `gix` writes are not yet stable enough for our needs.

The shell-out helper centralizes:
- Working directory set to `self.root`
- `GIT_TERMINAL_PROMPT=0` to fail rather than hang
- `--no-pager` for output predictability
- Capturing exit codes and stderr into `GitError::Shell`

### Detached HEAD

`is_detached` returns `true` when HEAD does not point at a branch. Several
Firetrail features (claim, write) refuse on detached HEAD; ft-git itself is
neutral and just reports state.

### Hook installation

Hooks are written to `.git/hooks/<name>` with `chmod 755`. If a hook already
exists, `install_hook` appends to a marked region:

```
# >>> firetrail managed >>>
<our content>
# <<< firetrail managed <<<
```

Repeated installs replace only the marked region. Manual user content outside
the markers is preserved.

The `PreReceiveProtectFiretrail` variant does not install locally — calling
`install_hook` with it writes the script to a documented path
(`.firetrail/hooks/pre-receive`) for the team to install on the Git server.

---

## Acceptance

1. `Repo::open` and `Repo::discover` work against a fresh repo (initialized by
   `ft-testkit::TestRepo`).
2. `head`, `current_branch`, `is_detached`, `is_clean`, `status` produce correct
   results across:
   - Fresh repo with one empty commit
   - Repo with uncommitted changes
   - Repo on a feature branch
   - Repo on detached HEAD
3. `read_file_at_ref` returns the bytes of a file at a named ref; returns
   `FileNotInTree` for a path not present at that ref.
4. `list_files_at_ref` honors a glob filter (e.g. `.firetrail/records/**/*.json`)
   and returns sorted paths.
5. `branch_create`, `branch_delete`, `checkout` work and reflect in subsequent
   `branches()` output.
6. `install_hook` writes an executable file with the managed-region markers;
   re-installing replaces only the markers.
7. Doc tests on every public method with at least one runnable example using
   `ft-testkit::TestRepo`.
8. Property tests for `list_files_at_ref` glob behavior.

---

## Testing requirements

- Unit tests use `tempfile::TempDir` and shell out to `git init` directly to
  avoid coupling to `ft-testkit`'s richer fixtures.
- Integration tests use `ft-testkit::TestRepo`.
- One test exercises a corrupted `.git/refs/heads/main` to assert `GitError::Gix`
  or `GitError::Shell` rather than panic.

---

## Out of scope

- Merge operations (handled by the system `git` and by the custom merge driver
  delivered in ft-pr at M4).
- Cherry-pick (ft-cli wraps it at higher level; ft-git just shells out when
  asked).
- Remote operations (push, pull, fetch) — added when ft-storage external mode
  lands at M5.
- Sub-modules — explicitly out of scope.

---

## References

- ADR-0002 — JSON-in-Git storage
- ADR-0016 — Build approach
- ADR-0017 — Audit chain integrity (force-push protection hook)
