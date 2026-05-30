//! Shared workspace path resolution for firetrail.
//!
//! A *workspace* is a git repository (the directory that contains `.git/`) that
//! also contains a `.firetrail/` directory once it has been initialised. This
//! crate owns the single canonical [`Workspace`] handle — the struct, its path
//! accessors, the `.firetrail/config.yml` initialised-marker check, and git-root
//! discovery — so that ft-cli, ft-ops, and ft-ui all agree on the layout instead
//! of each carrying a copy.
//!
//! ## Decoupling
//!
//! The fallible methods here return a self-contained [`WorkspaceError`] rather
//! than any transport-specific error (no `CliError`, no `OpsError`, no HTTP
//! status). Consumers convert at their boundary via `From<WorkspaceError>`
//! (ft-cli and ft-ops each provide one) so error ergonomics and messages are
//! preserved on each side.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors produced while resolving or opening a workspace.
///
/// The variant set deliberately mirrors the small shape ft-ops and ft-cli need
/// (`NotFound` / `Validation` / `Internal`) so their `From<WorkspaceError>`
/// adapters are mechanical and lossless.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// The workspace root (or a required entity) does not exist on disk.
    #[error("{entity} not found: {path}")]
    NotFound {
        /// What was being looked up (e.g. `"workspace"`).
        entity: String,
        /// The path that did not exist.
        path: String,
    },

    /// A workspace invariant was violated (e.g. not a git repo, missing
    /// `.firetrail/config.yml` marker).
    #[error("validation failed on `{field}`: {reason}")]
    Validation {
        /// Field / input the validation applies to (dot-path).
        field: String,
        /// Why it was rejected.
        reason: String,
    },

    /// An unexpected internal failure (e.g. could not resolve the machine-local
    /// cache home).
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl WorkspaceError {
    /// Construct a [`WorkspaceError::NotFound`] without ceremony.
    pub fn not_found(entity: impl Into<String>, path: impl Into<String>) -> Self {
        Self::NotFound {
            entity: entity.into(),
            path: path.into(),
        }
    }

    /// Construct a [`WorkspaceError::Validation`] without ceremony.
    pub fn validation(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

/// Resolved workspace paths for a firetrail repository.
///
/// A workspace is the directory containing both `.git/` and (once initialised)
/// `.firetrail/`. The accessors here are pure path joins — no I/O — except
/// [`Self::runtime_dir`] and [`Self::daemon_socket_path`], which delegate to
/// `ft_embed::repo_cache_dir` for the machine-local cache layout (ADR-0007).
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Absolute repository root (the directory that contains `.git/`).
    pub root: PathBuf,
}

impl Workspace {
    /// Open an existing firetrail workspace rooted at `root`.
    ///
    /// The root must exist and must contain a `.firetrail/config.yml` marker
    /// (the marker firetrail uses to decide a workspace is initialised).
    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(WorkspaceError::not_found(
                "workspace",
                root.display().to_string(),
            ));
        }
        let ws = Self {
            root: root.to_path_buf(),
        };
        if !ws.is_initialised() {
            return Err(WorkspaceError::validation(
                "workspace.root",
                format!(
                    "not an initialised firetrail workspace (missing {})",
                    ws.config_path().display()
                ),
            ));
        }
        Ok(ws)
    }

    /// Open `root` without requiring the `.firetrail/config.yml` marker.
    ///
    /// Useful for ops that operate on a fresh (uninitialised) repo. The
    /// directory must still exist.
    pub fn open_uninitialised(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(WorkspaceError::not_found(
                "workspace",
                root.display().to_string(),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Locate a workspace by walking up from `start` looking for `.git/`.
    ///
    /// The resulting root is returned regardless of whether `.firetrail/`
    /// exists yet (so `firetrail init` can run inside a fresh repo).
    pub fn locate(start: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let start = start.as_ref();
        let root = find_git_root(start).ok_or_else(|| {
            WorkspaceError::validation(
                "workspace.start",
                format!(
                    "not inside a git repository (searched upwards from {})",
                    start.display()
                ),
            )
        })?;
        Ok(Self { root })
    }

    /// Absolute path to the `.firetrail/` directory.
    #[must_use]
    pub fn firetrail_dir(&self) -> PathBuf {
        self.root.join(".firetrail")
    }

    /// Absolute path to `.firetrail/config.yml`.
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.firetrail_dir().join("config.yml")
    }

    /// Absolute path to `.firetrail/identity.yml`.
    #[must_use]
    pub fn identity_path(&self) -> PathBuf {
        self.firetrail_dir().join("identity.yml")
    }

    /// Absolute path to `.firetrail/index.db`.
    #[must_use]
    pub fn index_db_path(&self) -> PathBuf {
        self.firetrail_dir().join("index.db")
    }

    /// Absolute path to `.firetrail/cache/`.
    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.firetrail_dir().join("cache")
    }

    /// Machine-local runtime directory for this repo (ADR-0007). Lives under
    /// `$FIRETRAIL_CACHE_HOME/firetrail/<repo-hash>/` or
    /// `~/.cache/firetrail/<repo-hash>/`, shared with the embedding cache.
    ///
    /// This is **not** workspace-local: it sidesteps the macOS `SUN_LEN`
    /// limit (~104 chars) that long temp paths under
    /// `/private/var/folders/...` would otherwise blow past when binding the
    /// Unix domain socket (firetrail-tij).
    ///
    /// The repo-hash derivation lives in `ft_embed` (it is the same hash the
    /// embedding cache keys on), so we delegate rather than duplicate it here.
    pub fn runtime_dir(&self) -> Result<PathBuf, WorkspaceError> {
        ft_embed::repo_cache_dir(&self.root).map_err(|e| {
            WorkspaceError::Internal(anyhow::anyhow!("resolve machine-local runtime dir: {e}"))
        })
    }

    /// Default embedding daemon socket path. Lives under
    /// [`Self::runtime_dir`] to keep the path short on macOS.
    pub fn daemon_socket_path(&self) -> Result<PathBuf, WorkspaceError> {
        Ok(self.runtime_dir()?.join("embedd.sock"))
    }

    /// Whether the workspace has been initialised (the marker is
    /// `.firetrail/config.yml`).
    #[must_use]
    pub fn is_initialised(&self) -> bool {
        self.config_path().exists()
    }
}

/// Walk up from `start` looking for a directory that contains a `.git` entry.
///
/// Returns the first such directory (the repo root), or `None` if the search
/// reaches the filesystem root without finding one.
#[must_use]
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_repo(dir: &Path) {
        fs::create_dir_all(dir.join(".git")).unwrap();
    }

    fn initialise(dir: &Path) {
        fs::create_dir_all(dir.join(".firetrail")).unwrap();
        fs::write(dir.join(".firetrail").join("config.yml"), "version: 0\n").unwrap();
    }

    #[test]
    fn open_requires_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Without the marker, open must fail with a Validation error.
        let err = Workspace::open(root).expect_err("open should fail without marker");
        assert!(matches!(err, WorkspaceError::Validation { .. }), "{err:?}");

        // With the marker present, open succeeds.
        initialise(root);
        let ws = Workspace::open(root).expect("open should succeed with marker");
        assert_eq!(ws.root, root);
        assert!(ws.is_initialised());
    }

    #[test]
    fn open_missing_root_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let err = Workspace::open(&missing).expect_err("missing root should fail");
        assert!(matches!(err, WorkspaceError::NotFound { .. }), "{err:?}");
    }

    #[test]
    fn open_uninitialised_allows_fresh_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let ws = Workspace::open_uninitialised(root).expect("fresh repo opens");
        assert!(!ws.is_initialised());
        assert_eq!(ws.root, root);
    }

    #[test]
    fn path_accessors_join_under_firetrail() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let ws = Workspace::open_uninitialised(root).unwrap();
        let dot = root.join(".firetrail");
        assert_eq!(ws.firetrail_dir(), dot);
        assert_eq!(ws.config_path(), dot.join("config.yml"));
        assert_eq!(ws.identity_path(), dot.join("identity.yml"));
        assert_eq!(ws.index_db_path(), dot.join("index.db"));
        assert_eq!(ws.cache_dir(), dot.join("cache"));
    }

    #[test]
    fn locate_walks_up_to_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        make_repo(root);
        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        let ws = Workspace::locate(&nested).expect("locate from nested dir");
        assert_eq!(ws.root, root);
    }

    #[test]
    fn locate_outside_repo_is_validation_error() {
        let tmp = tempfile::tempdir().unwrap();
        // No .git anywhere under tmp.
        let err = Workspace::locate(tmp.path()).expect_err("no git root should fail");
        assert!(matches!(err, WorkspaceError::Validation { .. }), "{err:?}");
    }

    #[test]
    fn daemon_socket_path_lives_under_runtime_dir() {
        // `runtime_dir` reads `$FIRETRAIL_CACHE_HOME`/`$HOME`; the crate forbids
        // `unsafe`, so we cannot mutate the environment. Instead we assert the
        // structural invariant: when resolution succeeds, the socket path is
        // exactly `<runtime_dir>/embedd.sock`. (env var mutation is exercised in
        // ft-embed's own tests, which own the cache-home resolution.)
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let ws = Workspace::open_uninitialised(&repo).unwrap();

        if let Ok(runtime) = ws.runtime_dir() {
            let socket = ws.daemon_socket_path().expect("socket path resolves");
            assert_eq!(socket, runtime.join("embedd.sock"));
            assert!(socket.ends_with("embedd.sock"));
        }
    }
}
