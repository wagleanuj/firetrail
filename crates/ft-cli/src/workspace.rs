//! Workspace path discovery for the CLI.
//!
//! The [`Workspace`] handle (struct + path accessors) now lives in the shared
//! `ft-workspace` crate (extracted in firetrail-jyc); this module re-exports it
//! and keeps the CLI-flavoured discovery helpers ([`locate`],
//! [`require_initialised`]) that translate workspace-resolution failures into
//! [`CliError`] with the exact command-framed messages the CLI surfaces.
//!
//! A *workspace* is a git repository that contains a `.firetrail/` directory.
//! Commands either accept `--workspace <path>` or discover the workspace by
//! walking up from the current directory.

use std::path::{Path, PathBuf};

use crate::error::CliError;

pub use ft_workspace::Workspace;

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

    let root = ft_workspace::find_git_root(&start).ok_or_else(|| CliError::UserError {
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
