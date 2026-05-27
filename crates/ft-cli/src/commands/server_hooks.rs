//! `firetrail server-hooks install --dest <path>` — copy server-side hook
//! templates into a destination directory.
//!
//! The hook source ships in this binary as a compiled-in string (via
//! `include_str!`) so that the installer works regardless of the workspace
//! layout. The destination directory is created on demand.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::cli::{GlobalOpts, ServerHooksInstallArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;

const COMMAND: &str = "server-hooks install";

/// `pre-receive` hook shipped with the binary. Mirrors
/// `templates/hooks/pre-receive` at the workspace root.
const PRE_RECEIVE: &str = include_str!("../../../../templates/hooks/pre-receive");

/// `firetrail server-hooks install`
pub fn install(
    args: &ServerHooksInstallArgs,
    _global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let dest = &args.dest;
    fs::create_dir_all(dest)
        .map_err(|e| CliError::internal(COMMAND, format!("create {}: {e}", dest.display())))?;

    let mut installed: Vec<InstalledHook> = Vec::new();

    let pre_receive_path = dest.join("pre-receive");
    fs::write(&pre_receive_path, PRE_RECEIVE).map_err(|e| {
        CliError::internal(
            COMMAND,
            format!("write {}: {e}", pre_receive_path.display()),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&pre_receive_path)
            .map_err(|e| {
                CliError::internal(COMMAND, format!("stat {}: {e}", pre_receive_path.display()))
            })?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&pre_receive_path, perms).map_err(|e| {
            CliError::internal(
                COMMAND,
                format!("chmod {}: {e}", pre_receive_path.display()),
            )
        })?;
    }
    installed.push(InstalledHook {
        name: "pre-receive".into(),
        path: pre_receive_path,
    });

    Ok(CommandOutcome::ServerHooks(ServerHooksOutcome {
        dest: dest.clone(),
        installed,
        warnings: Vec::new(),
    }))
}

/// One installed hook entry.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledHook {
    /// Hook file name.
    pub name: String,
    /// Absolute or workspace-relative destination path.
    pub path: PathBuf,
}

/// Outcome of `server-hooks install`.
#[derive(Debug, Clone, Serialize)]
pub struct ServerHooksOutcome {
    /// Destination directory.
    pub dest: PathBuf,
    /// Per-hook install records.
    pub installed: Vec<InstalledHook>,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ServerHooksOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# server-hooks install\n\nDestination: `{}`\n\nInstalled:\n",
            self.dest.display()
        );
        for h in &self.installed {
            let _ = writeln!(s, "- `{}` → `{}`", h.name, h.path.display());
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "server-hooks install: {} hook(s) → {}",
            self.installed.len(),
            self.dest.display()
        )
    }
}
