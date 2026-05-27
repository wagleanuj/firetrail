//! `firetrail sync` — explicit pull + push for external storage mode (M5).

use std::fmt::Write as _;

use ft_storage::{ExternalStorage, StorageMode, sync_status};
use serde::Serialize;

use crate::cli::{GlobalOpts, SyncArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;

const COMMAND: &str = "sync";

/// Outcome of `firetrail sync`.
#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Whether the pull step ran.
    pub pulled: bool,
    /// Whether the push step ran.
    pub pushed: bool,
    /// Local-vs-remote status snapshot taken after sync.
    pub status: Option<SyncStatusView>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Serializable view of [`ft_storage::SyncStatus`].
#[derive(Debug, Clone, Serialize)]
pub struct SyncStatusView {
    /// Commits ahead of the remote.
    pub ahead: usize,
    /// Commits behind the remote.
    pub behind: usize,
    /// Dirty working tree flag.
    pub dirty: bool,
}

impl SyncOutcome {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut s = format!("**sync**: pulled={} pushed={}\n", self.pulled, self.pushed);
        if let Some(st) = &self.status {
            let _ = writeln!(
                s,
                "ahead={} behind={} dirty={}",
                st.ahead, st.behind, st.dirty
            );
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("sync pulled={} pushed={}", self.pulled, self.pushed)
    }
}

/// Entry point.
pub fn run(args: &SyncArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = crate::workspace::require_initialised(COMMAND, global.workspace.as_deref())?;
    let mode = StorageMode::from_workspace(&ws.root)
        .map_err(|e| CliError::internal(COMMAND, format!("read storage config: {e}")))?;
    let StorageMode::External { config, .. } = mode else {
        return Err(CliError::user(
            COMMAND,
            "sync is only meaningful in external storage mode",
        ));
    };
    let ext = ExternalStorage::open(&ws.root, &config)
        .map_err(|e| CliError::internal(COMMAND, format!("open external storage: {e}")))?;

    let mut warnings = Vec::new();
    let do_pull = !args.push_only;
    let do_push = !args.pull_only;

    if do_pull {
        if let Err(e) = ext.pull() {
            warnings.push(format!("pull failed: {e}"));
        }
    }
    if do_push {
        if let Err(e) = ext.push() {
            warnings.push(format!("push failed: {e}"));
        }
    }

    let status = match sync_status(&ext) {
        Ok(st) => Some(SyncStatusView {
            ahead: st.ahead,
            behind: st.behind,
            dirty: st.dirty,
        }),
        Err(e) => {
            warnings.push(format!("sync_status: {e}"));
            None
        }
    };

    Ok(CommandOutcome::Sync(SyncOutcome {
        command: COMMAND,
        pulled: do_pull,
        pushed: do_push,
        status,
        warnings,
    }))
}
