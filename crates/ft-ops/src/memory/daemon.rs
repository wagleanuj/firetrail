//! Embed-daemon auto-spawn helper for `memory::search`.
//!
//! Mirrors `ft_cli::commands::daemon_cmd::ensure_running` so semantic /
//! hybrid search can pick up a fresh daemon without the operator running
//! `firetrail daemon start` first. The original lives in ft-cli; we copy
//! the spawn logic rather than depend on ft-cli (ft-ops sits below it).
//!
//! Unix-only. On non-Unix the helper short-circuits and returns the
//! current daemon status without trying to spawn anything; the caller
//! degrades to lexical search.
//
// FIXME(firetrail-xy6): when ft-cli's search command rewires onto
// `ft_ops::memory::search`, drop ft-cli's `ensure_running` and have it
// call into this helper instead (probably promoted to `pub` at that
// point and moved to a shared location).

use std::path::Path;
use std::time::{Duration, Instant};

use ft_embed::DaemonStatus;
use ft_embed::daemon as embed_daemon;

use crate::error::OpsError;
use crate::workspace::Workspace;

/// How long to wait for a freshly spawned daemon to come up.
const SPAWN_WAIT: Duration = Duration::from_secs(5);

/// If the daemon for `ws` is not running, spawn a detached one and wait
/// for it to start listening. Returns the resulting [`DaemonStatus`].
///
/// Callers must handle `Stopped`/`Unreachable` (typically: push a warning
/// and fall back to lexical search). The function never returns an error
/// for a non-running daemon — only for genuinely unrecoverable I/O.
pub(crate) fn ensure_running(ws: &Workspace) -> Result<DaemonStatus, OpsError> {
    let socket = ws.daemon_socket_path()?;
    match embed_daemon::status(&socket) {
        DaemonStatus::Running => return Ok(DaemonStatus::Running),
        DaemonStatus::Unreachable => return Ok(DaemonStatus::Unreachable),
        DaemonStatus::Stopped => {}
    }

    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("create runtime dir: {e}")))?;
    }

    if !spawn_detached(ws, &socket) {
        // Spawn failed (or non-Unix); report the current status without
        // raising so callers degrade to lexical.
        return Ok(embed_daemon::status(&socket));
    }

    if wait_for(
        || embed_daemon::status(&socket) == DaemonStatus::Running,
        SPAWN_WAIT,
    ) {
        Ok(DaemonStatus::Running)
    } else {
        Ok(embed_daemon::status(&socket))
    }
}

#[cfg(unix)]
fn spawn_detached(ws: &Workspace, socket: &Path) -> bool {
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else {
        tracing::warn!(
            op = "memory::search",
            "current_exe unavailable; cannot spawn daemon"
        );
        return false;
    };
    let workspace_arg = ws.root.display().to_string();
    let socket_arg = socket.display().to_string();
    let mut cmd = Command::new(exe);
    cmd.arg("--workspace")
        .arg(&workspace_arg)
        .arg("daemon")
        .arg("start")
        .arg("--foreground")
        .arg("--socket")
        .arg(&socket_arg)
        .arg("--idle-timeout-secs")
        .arg("300")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.spawn() {
        Ok(child) => {
            // Forget the child so we don't reap it on drop; the daemon
            // lives independently.
            std::mem::forget(child);
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "spawn embed daemon");
            false
        }
    }
}

#[cfg(not(unix))]
fn spawn_detached(_ws: &Workspace, _socket: &Path) -> bool {
    tracing::warn!("background daemon spawn is Unix-only");
    false
}

fn wait_for(predicate: impl Fn() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    predicate()
}
