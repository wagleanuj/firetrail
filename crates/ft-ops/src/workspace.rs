//! Workspace handle used by every op.
//!
//! Mirrors ft-cli's `crate::workspace::Workspace` (paths, accessors, initialised
//! check, git-root discovery). The duplication is deliberate for Wave 1-A:
//! pulling ft-cli's struct into ft-ops would require rewiring CliError-returning
//! methods in ft-cli's command bodies (option (a) in the W1-A design), which is
//! explicitly out of scope until the CLI rewire follow-up lands.
//!
// FIXME(W1-A-follow): once ft-cli's ticket commands move onto ft-ops::tickets,
// drop ft-cli's `workspace.rs` and have it re-export from here. Tracked in the
// "rewire ft-cli ticket commands to call ft-ops" beads issue filed alongside
// firetrail-bhj.

use std::path::{Path, PathBuf};

use crate::error::OpsError;

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
    /// (the same marker ft-cli uses to decide a workspace is initialised).
    pub fn open(root: impl AsRef<Path>) -> Result<Self, OpsError> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(OpsError::not_found(
                "workspace",
                root.display().to_string(),
            ));
        }
        let ws = Self {
            root: root.to_path_buf(),
        };
        if !ws.is_initialised() {
            return Err(OpsError::validation(
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
    pub fn open_uninitialised(root: impl AsRef<Path>) -> Result<Self, OpsError> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(OpsError::not_found(
                "workspace",
                root.display().to_string(),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Locate a workspace by walking up from `start` looking for `.git/`.
    pub fn locate(start: impl AsRef<Path>) -> Result<Self, OpsError> {
        let start = start.as_ref();
        let root = find_git_root(start).ok_or_else(|| {
            OpsError::validation(
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

    /// Machine-local runtime directory for this repo (ADR-0007).
    pub fn runtime_dir(&self) -> Result<PathBuf, OpsError> {
        ft_embed::repo_cache_dir(&self.root).map_err(|e| {
            OpsError::Internal(anyhow::anyhow!("resolve machine-local runtime dir: {e}"))
        })
    }

    /// Default embedding daemon socket path.
    pub fn daemon_socket_path(&self) -> Result<PathBuf, OpsError> {
        Ok(self.runtime_dir()?.join("embedd.sock"))
    }

    /// Whether the workspace has been initialised (the marker is
    /// `.firetrail/config.yml`).
    #[must_use]
    pub fn is_initialised(&self) -> bool {
        self.config_path().exists()
    }
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}
