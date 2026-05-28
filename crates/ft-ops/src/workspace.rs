//! Workspace handle used by every op.
//!
//! For Wave 0 this is a minimal stub: it holds the absolute repo root and
//! verifies that a `.firetrail/config.yml` marker exists on construction.
//!
// FIXME(firetrail-ops): consolidate workspace resolution. ft-cli has a richer
// `Workspace` (firetrail_dir, identity_path, runtime_dir, etc.) tangled with
// `CliError`. Either extract a shared `ft-workspace` crate or have ft-cli's
// workspace re-export a Wave-1 trait that ft-ops consumes. Tracked separately.

use std::path::{Path, PathBuf};

use crate::error::OpsError;

/// Resolved workspace paths for a firetrail repository.
///
/// A workspace is the directory containing both `.git/` and `.firetrail/`. For
/// now this struct exposes only the root; richer accessors (config path,
/// runtime dir, identity path) will land when the workspace logic is
/// consolidated across ft-cli and ft-ops.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Absolute repository root.
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
        let marker = root.join(".firetrail").join("config.yml");
        if !marker.exists() {
            return Err(OpsError::validation(
                "workspace.root",
                format!(
                    "not an initialised firetrail workspace (missing {})",
                    marker.display()
                ),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Absolute path to the `.firetrail/` directory.
    #[must_use]
    pub fn firetrail_dir(&self) -> PathBuf {
        self.root.join(".firetrail")
    }
}
