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

use std::path::{Path, PathBuf};
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

/// Name of the CLI binary that hosts the `daemon start` subcommand.
const DAEMON_BIN: &str = "firetrail";

/// Resolve the `firetrail` binary that hosts `daemon start`.
///
/// The embed daemon is a subcommand of the main `firetrail` CLI, **not** of
/// every binary that links ft-ops. When the caller is the web server
/// (`ft-ui`), `std::env::current_exe()` points at `ft-ui`, which has no
/// `daemon` subcommand: spawning it errors out instantly (`unexpected argument
/// 'daemon'`), yet `Command::spawn` still reports success, so [`ensure_running`]
/// would wait the full [`SPAWN_WAIT`] for a socket that never appears. To avoid
/// that 5-second stall we locate the real binary instead of blindly reusing
/// `current_exe`:
///
/// 1. if we *are* `firetrail`, reuse the current exe;
/// 2. otherwise look for a sibling `firetrail` in the same directory — cargo
///    lays every binary in one `target/<profile>/` dir, and installed layouts
///    keep the CLI and UI in the same bindir.
///
/// Returns `None` when no such binary exists, signalling the caller to skip the
/// spawn and degrade to the mock embedder immediately rather than block.
fn resolve_daemon_exe_from(current: &Path) -> Option<PathBuf> {
    if current.file_name().is_some_and(|n| n == DAEMON_BIN) {
        return Some(current.to_path_buf());
    }
    let sibling = current.parent()?.join(DAEMON_BIN);
    sibling.exists().then_some(sibling)
}

/// [`resolve_daemon_exe_from`] applied to the current executable.
fn resolve_daemon_exe() -> Option<PathBuf> {
    resolve_daemon_exe_from(&std::env::current_exe().ok()?)
}

#[cfg(unix)]
fn spawn_detached(ws: &Workspace, socket: &Path) -> bool {
    use std::process::{Command, Stdio};
    let Some(exe) = resolve_daemon_exe() else {
        tracing::warn!(
            op = "memory::search",
            "no `{DAEMON_BIN}` binary found next to the current executable; \
             skipping embed-daemon auto-spawn (degrading to mock/lexical)"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_uses_current_exe_when_it_is_firetrail() {
        // The CLI path: current_exe is already `firetrail` → reuse it verbatim,
        // existence not required (we are that running process).
        let p = Path::new("/opt/firetrail/bin/firetrail");
        assert_eq!(resolve_daemon_exe_from(p), Some(p.to_path_buf()));
    }

    #[test]
    fn resolve_finds_sibling_firetrail() {
        // The ft-ui path: current_exe is `ft-ui`; a sibling `firetrail` in the
        // same dir is the daemon host.
        let dir = tempfile::tempdir().unwrap();
        let firetrail = dir.path().join(DAEMON_BIN);
        std::fs::write(&firetrail, b"#!/bin/sh\n").unwrap();
        let ft_ui = dir.path().join("ft-ui");
        std::fs::write(&ft_ui, b"#!/bin/sh\n").unwrap();
        assert_eq!(resolve_daemon_exe_from(&ft_ui), Some(firetrail));
    }

    #[test]
    fn resolve_returns_none_without_sibling() {
        // No `firetrail` next to us → caller must skip the doomed spawn and
        // degrade immediately instead of waiting out SPAWN_WAIT.
        let dir = tempfile::tempdir().unwrap();
        let ft_ui = dir.path().join("ft-ui");
        std::fs::write(&ft_ui, b"#!/bin/sh\n").unwrap();
        assert_eq!(resolve_daemon_exe_from(&ft_ui), None);
    }
}
