//! Workspace path discovery.
//!
//! A *workspace* is a git repository that contains a `.firetrail/` directory.
//! Commands either accept `--workspace <path>` or discover the workspace by
//! walking up from the current directory.

use std::path::{Path, PathBuf};

use crate::error::CliError;

/// Resolved workspace paths.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Absolute repo root (the directory that contains `.git/`).
    pub root: PathBuf,
}

impl Workspace {
    /// Absolute path to `.firetrail/`.
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

    /// Absolute path to `.firetrail/sockets/`.
    #[must_use]
    pub fn sockets_dir(&self) -> PathBuf {
        self.firetrail_dir().join("sockets")
    }

    /// Default embedding daemon socket path.
    #[must_use]
    pub fn daemon_socket_path(&self) -> PathBuf {
        self.sockets_dir().join("embedd.sock")
    }

    /// Absolute path to `.firetrail/cache/`.
    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.firetrail_dir().join("cache")
    }

    /// Whether the workspace has been initialised (the marker is
    /// `.firetrail/config.yml`).
    #[must_use]
    pub fn is_initialised(&self) -> bool {
        self.config_path().exists()
    }
}

/// Locate the workspace root. If `override_path` is set, it is used directly
/// (no discovery). Otherwise we walk up from the current directory looking
/// for a `.git` entry; the resulting path is the workspace root regardless
/// of whether `.firetrail/` exists yet (so `firetrail init` can run inside
/// a fresh repo).
pub fn locate(command: &str, override_path: Option<&Path>) -> Result<Workspace, CliError> {
    let start = if let Some(p) = override_path {
        canonicalize(command, p)?
    } else {
        std::env::current_dir().map_err(|e| CliError::internal(command, e))?
    };

    let root = find_git_root(&start).ok_or_else(|| CliError::UserError {
        command: command.to_string(),
        message: format!(
            "not inside a git repository (searched upwards from {})",
            start.display()
        ),
        details: serde_json::json!({ "start": start.display().to_string() }),
    })?;

    Ok(Workspace { root })
}

/// Like [`locate`] but additionally enforces that `.firetrail/` exists and
/// has been initialised.
pub fn require_initialised(
    command: &str,
    override_path: Option<&Path>,
) -> Result<Workspace, CliError> {
    let ws = locate(command, override_path)?;
    if !ws.is_initialised() {
        return Err(CliError::NotInitialized {
            command: command.to_string(),
            path: ws.firetrail_dir(),
        });
    }
    Ok(ws)
}

fn canonicalize(command: &str, p: &Path) -> Result<PathBuf, CliError> {
    p.canonicalize().map_err(|e| CliError::UserError {
        command: command.to_string(),
        message: format!("workspace path {} unusable: {e}", p.display()),
        details: serde_json::json!({ "path": p.display().to_string() }),
    })
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
