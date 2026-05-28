//! `firetrail daemon {start,stop,status}` — embedding daemon control.
//!
//! M3 scope and limitations:
//!
//! - `daemon start --foreground` runs the daemon in this process. Tests use
//!   this surface (with a unique socket path) so they can deterministically
//!   stand a daemon up.
//! - `daemon start` (no `--foreground`) spawns the current binary with
//!   `--foreground` as a detached child via the standard library. This is
//!   intentionally minimal — full lifecycle / supervision is ADR-0007 work.
//!   Unix-only at M3; on non-Unix the call returns a user error suggesting
//!   `--foreground`.
//! - `daemon stop` deletes the socket file. The serving loop sees the next
//!   `accept()` fail and exits. A richer shutdown frame is filed as a
//!   follow-up.
//! - `daemon status` is a thin wrapper around `ft_embed::daemon::status`.

use std::path::Path;
use std::process::Command;

use ft_embed::daemon::{self, DaemonStatus};
use ft_embed::{Embedder, EmbedService, EmbeddingCache, EmbeddingsConfig, build_embedder};
use serde::Serialize;

use crate::cli::{DaemonStartArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const CMD_START: &str = "daemon start";
const CMD_STOP: &str = "daemon stop";
const CMD_STATUS: &str = "daemon status";

/// `firetrail daemon start`
pub fn start(args: &DaemonStartArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_START, global.workspace.as_deref())?;
    let socket = args
        .socket
        .clone()
        .unwrap_or_else(|| ws.daemon_socket_path());
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::internal(CMD_START, e))?;
    }

    if args.foreground {
        return run_foreground(&ws.root, &socket);
    }

    // Background mode: spawn the current binary with --foreground. Unix only.
    #[cfg(unix)]
    {
        let exe = std::env::current_exe()
            .map_err(|e| CliError::internal(CMD_START, format!("current_exe: {e}")))?;
        let workspace_arg = ws.root.display().to_string();
        let socket_arg = socket.display().to_string();
        let mut cmd = Command::new(exe);
        cmd.arg("--workspace")
            .arg(&workspace_arg)
            .arg("daemon")
            .arg("start")
            .arg("--foreground")
            .arg("--socket")
            .arg(&socket_arg);
        // Detach: redirect std streams to /dev/null.
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        let child = cmd
            .spawn()
            .map_err(|e| CliError::internal(CMD_START, format!("spawn daemon: {e}")))?;
        let pid = child.id();
        // Intentionally drop the Child without waiting — the OS reaps it when
        // the parent dies. A richer supervisor is a follow-up.
        std::mem::forget(child);
        Ok(CommandOutcome::Daemon(DaemonOutcome {
            command: CMD_START,
            socket: socket.display().to_string(),
            status: "spawned".to_string(),
            pid: Some(pid),
            warnings: vec![
                "background spawn is best-effort at M3; check `firetrail daemon status` shortly"
                    .to_string(),
            ],
        }))
    }
    #[cfg(not(unix))]
    {
        let _ = socket;
        Err(CliError::user(
            CMD_START,
            "background daemon spawn is Unix-only at M3; run with --foreground instead",
        ))
    }
}

fn run_foreground(ws_root: &Path, socket: &Path) -> Result<CommandOutcome, CliError> {
    // Resolve the configured embedder (firetrail-6n4): `embeddings:` in
    // `.firetrail/config.yml` selects `local` (ONNX), `mock`, or `lexical`.
    // `lexical` means there is nothing to serve — fail loudly so operators
    // notice rather than running a no-op daemon.
    let cfg = EmbeddingsConfig::from_workspace(ws_root)
        .map_err(|e| CliError::internal(CMD_START, format!("load embeddings config: {e}")))?;
    let built = build_embedder(&cfg)
        .map_err(|e| CliError::internal(CMD_START, format!("build embedder: {e}")))?;
    let warnings = built.warnings;
    let embedder: Box<dyn Embedder> = built.embedder.ok_or_else(|| {
        CliError::user(
            CMD_START,
            "embeddings.provider=lexical: nothing to serve via the daemon",
        )
    })?;

    let cache = EmbeddingCache::open_under(ws_root)
        .map_err(|e| CliError::internal(CMD_START, format!("open cache: {e}")))?;
    let service = EmbedService::new(embedder, cache);
    daemon::serve(socket, &service)
        .map_err(|e| CliError::internal(CMD_START, format!("daemon serve: {e}")))?;
    // `serve` only returns when the listener exits.
    Ok(CommandOutcome::Daemon(DaemonOutcome {
        command: CMD_START,
        socket: socket.display().to_string(),
        status: "exited".to_string(),
        pid: None,
        warnings,
    }))
}

/// `firetrail daemon stop`
pub fn stop(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_STOP, global.workspace.as_deref())?;
    let socket = ws.daemon_socket_path();
    let existed = socket.exists();
    if existed {
        // Removing the socket file makes the next `accept()` fail with EBADF
        // (or ENOENT depending on the platform); the serve loop logs and
        // continues but new connections error out. A future shutdown frame
        // is filed as a follow-up.
        std::fs::remove_file(&socket)
            .map_err(|e| CliError::internal(CMD_STOP, format!("remove socket: {e}")))?;
    }
    Ok(CommandOutcome::Daemon(DaemonOutcome {
        command: CMD_STOP,
        socket: socket.display().to_string(),
        status: if existed { "stopped" } else { "absent" }.to_string(),
        pid: None,
        warnings: if existed {
            Vec::new()
        } else {
            vec!["no socket file present; daemon may already be stopped".to_string()]
        },
    }))
}

/// `firetrail daemon status`
pub fn status(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_STATUS, global.workspace.as_deref())?;
    let socket = ws.daemon_socket_path();
    let status = daemon::status(&socket);
    Ok(CommandOutcome::Daemon(DaemonOutcome {
        command: CMD_STATUS,
        socket: socket.display().to_string(),
        status: status_label(status).to_string(),
        pid: None,
        warnings: Vec::new(),
    }))
}

fn status_label(s: DaemonStatus) -> &'static str {
    match s {
        DaemonStatus::Running => "running",
        DaemonStatus::Stopped => "stopped",
        DaemonStatus::Unreachable => "unreachable",
    }
}

/// JSON / markdown view of a daemon command result.
#[derive(Debug, Clone, Serialize)]
pub struct DaemonOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Socket path the daemon binds to.
    pub socket: String,
    /// One-word status label (`running`, `stopped`, `unreachable`,
    /// `spawned`, `absent`, `exited`).
    pub status: String,
    /// Child PID for `start`'s background spawn (Unix only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl DaemonOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        let pid = self.pid.map(|p| format!(" pid={p}")).unwrap_or_default();
        format!(
            "**{}** socket=`{}` status={}{}\n",
            self.command, self.socket, self.status, pid
        )
    }
    /// One-line quiet summary.
    pub fn quiet_line(&self) -> String {
        format!("{}: {}", self.command, self.status)
    }
}
