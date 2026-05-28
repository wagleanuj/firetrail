//! `firetrail daemon {start,stop,status}` — embedding daemon control.
//!
//! Behaviour (post-firetrail-e7z):
//!
//! - `daemon start --foreground` runs the daemon in this process and holds
//!   an exclusive per-workspace lock so a second daemon for the same repo
//!   fails fast with a clear message.
//! - `daemon start` (no `--foreground`) spawns the current binary with
//!   `--foreground` as a detached child. Unix-only; non-Unix returns a user
//!   error suggesting `--foreground`.
//! - `daemon stop` opens the socket, sends a [`ft_embed::EmbedRequest::Shutdown`]
//!   frame, then polls until the socket disappears. Falls back to deleting
//!   the socket file if the daemon never acks.
//! - `daemon status` is a thin wrapper around [`ft_embed::daemon::status`].
//! - [`ensure_running`] is used by `search`/`similar` to auto-spawn a
//!   detached daemon when none is listening.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ft_core::RecordId;
use ft_embed::daemon::{self, DaemonStatus, RecordIndexer, ServeOptions};
use ft_embed::{EmbedService, Embedder, EmbeddingCache, EmbeddingsConfig, build_embedder};
use ft_search::SearchEngine;
use serde::Serialize;

use crate::cli::{DaemonStartArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace::{self, Workspace};

const CMD_START: &str = "daemon start";
const CMD_STOP: &str = "daemon stop";
const CMD_STATUS: &str = "daemon status";

/// How long `daemon stop` waits for a graceful exit before falling back.
const STOP_WAIT: Duration = Duration::from_secs(3);
/// How long `ensure_running` waits for a freshly spawned daemon to come up.
const SPAWN_WAIT: Duration = Duration::from_secs(5);

/// `firetrail daemon start`
pub fn start(args: &DaemonStartArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_START, global.workspace.as_deref())?;
    let socket = match args.socket.clone() {
        Some(p) => p,
        None => ws.daemon_socket_path()?,
    };
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::internal(CMD_START, e))?;
    }

    if args.foreground {
        return run_foreground(&ws, &socket, args.idle_timeout_secs);
    }

    spawn_background(&ws, &socket, args.idle_timeout_secs)
}

#[cfg(unix)]
fn spawn_background(
    ws: &Workspace,
    socket: &Path,
    idle_timeout_secs: u64,
) -> Result<CommandOutcome, CliError> {
    // Probe the embedder config in the parent so any fallback warnings
    // (most commonly "local embedder unavailable, falling back to mock"
    // when the build lacks the `onnx` feature or the model isn't on disk)
    // reach the operator instead of being swallowed by the detached child's
    // null stderr. We discard the embedder itself; the child will rebuild.
    let warnings = match EmbeddingsConfig::from_workspace(&ws.root) {
        Ok(cfg) => match build_embedder(&cfg) {
            Ok(b) => b.warnings,
            Err(e) => vec![format!(
                "could not preflight embedder ({e}); spawning anyway"
            )],
        },
        Err(e) => vec![format!("could not load embeddings config ({e})")],
    };

    let exe = std::env::current_exe()
        .map_err(|e| CliError::internal(CMD_START, format!("current_exe: {e}")))?;
    let workspace_arg = ws.root.display().to_string();
    let socket_arg = socket.display().to_string();
    let idle = idle_timeout_secs.to_string();
    let mut cmd = Command::new(exe);
    cmd.arg("--workspace")
        .arg(&workspace_arg)
        .arg("daemon")
        .arg("start")
        .arg("--foreground")
        .arg("--socket")
        .arg(&socket_arg)
        .arg("--idle-timeout-secs")
        .arg(&idle);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let child = cmd
        .spawn()
        .map_err(|e| CliError::internal(CMD_START, format!("spawn daemon: {e}")))?;
    let pid = child.id();
    std::mem::forget(child);
    Ok(CommandOutcome::Daemon(DaemonOutcome {
        command: CMD_START,
        socket: socket.display().to_string(),
        status: "spawned".to_string(),
        pid: Some(pid),
        warnings,
    }))
}

#[cfg(not(unix))]
fn spawn_background(
    _ws: &Workspace,
    _socket: &Path,
    _idle_timeout_secs: u64,
) -> Result<CommandOutcome, CliError> {
    Err(CliError::user(
        CMD_START,
        "background daemon spawn is Unix-only at M3; run with --foreground instead",
    ))
}

fn run_foreground(
    ws: &Workspace,
    socket: &Path,
    idle_timeout_secs: u64,
) -> Result<CommandOutcome, CliError> {
    let cfg = EmbeddingsConfig::from_workspace(&ws.root)
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

    let cache = EmbeddingCache::open_under(&ws.root)
        .map_err(|e| CliError::internal(CMD_START, format!("open cache: {e}")))?;
    let service = EmbedService::new(embedder, cache);

    let indexer: Arc<dyn RecordIndexer> = Arc::new(
        SearchEngineIndexer::open(&ws.index_db_path())
            .map_err(|e| CliError::internal(CMD_START, format!("open search index: {e}")))?,
    );
    let opts = ServeOptions {
        idle_timeout: (idle_timeout_secs > 0).then(|| Duration::from_secs(idle_timeout_secs)),
        lock_path: Some(daemon_lock_path(ws)?),
        indexer: Some(indexer),
    };
    daemon::serve_with(socket, &service, &opts)
        .map_err(|e| CliError::internal(CMD_START, format!("daemon serve: {e}")))?;
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
    let socket = ws.daemon_socket_path()?;

    if !socket.exists() {
        return Ok(CommandOutcome::Daemon(DaemonOutcome {
            command: CMD_STOP,
            socket: socket.display().to_string(),
            status: "absent".to_string(),
            pid: None,
            warnings: vec!["no socket file present; daemon may already be stopped".to_string()],
        }));
    }

    let mut warnings = Vec::new();
    let mut status_label = "stopped";
    match daemon::send_shutdown(&socket) {
        Ok(()) => {
            if !wait_for(|| !socket.exists(), STOP_WAIT) {
                warnings.push(
                    "daemon acked shutdown but socket file is still present; removing".to_string(),
                );
                let _ = std::fs::remove_file(&socket);
            }
        }
        Err(e) => {
            warnings.push(format!(
                "graceful shutdown failed ({e}); removing socket file as fallback"
            ));
            if let Err(rm) = std::fs::remove_file(&socket) {
                return Err(CliError::internal(CMD_STOP, format!("remove socket: {rm}")));
            }
            status_label = "force-stopped";
        }
    }

    Ok(CommandOutcome::Daemon(DaemonOutcome {
        command: CMD_STOP,
        socket: socket.display().to_string(),
        status: status_label.to_string(),
        pid: None,
        warnings,
    }))
}

/// `firetrail daemon status`
pub fn status(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_STATUS, global.workspace.as_deref())?;
    let socket = ws.daemon_socket_path()?;
    let status = daemon::status(&socket);
    Ok(CommandOutcome::Daemon(DaemonOutcome {
        command: CMD_STATUS,
        socket: socket.display().to_string(),
        status: status_label(status).to_string(),
        pid: None,
        warnings: Vec::new(),
    }))
}

/// If the daemon for `ws` is not running, spawn a detached one and wait for
/// it to start listening. Returns the current [`DaemonStatus`].
///
/// Caller is responsible for handling `Unreachable` (typically: report a
/// warning and fall back to lexical search).
pub fn ensure_running(_command: &'static str, ws: &Workspace) -> Result<DaemonStatus, CliError> {
    let socket = ws.daemon_socket_path()?;
    match daemon::status(&socket) {
        DaemonStatus::Running => return Ok(DaemonStatus::Running),
        DaemonStatus::Unreachable => return Ok(DaemonStatus::Unreachable),
        DaemonStatus::Stopped => {}
    }

    // The runtime dir may not exist on first invocation (it is no longer
    // created by `firetrail init` — it lives outside the workspace under
    // `~/.cache/firetrail/<repo-hash>/`).
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::internal("daemon ensure_running", e))?;
    }

    spawn_background(ws, &socket, 300)?;
    if wait_for(
        || daemon::status(&socket) == DaemonStatus::Running,
        SPAWN_WAIT,
    ) {
        Ok(DaemonStatus::Running)
    } else {
        Ok(daemon::status(&socket))
    }
}

/// [`RecordIndexer`] adapter that writes vectors into the workspace's
/// `ft-search` engine (firetrail-0nu).
pub struct SearchEngineIndexer {
    engine: Mutex<SearchEngine>,
}

impl SearchEngineIndexer {
    /// Open the search engine at `index_db_path`. Schema is ensured so the
    /// `vec0` table exists before the first upsert.
    pub fn open(index_db_path: &Path) -> Result<Self, ft_search::SearchError> {
        let engine = SearchEngine::open(index_db_path)?;
        engine.ensure_schema()?;
        Ok(Self {
            engine: Mutex::new(engine),
        })
    }
}

impl RecordIndexer for SearchEngineIndexer {
    fn upsert_vector(&self, record_id: &str, embedding: &[f32]) -> Result<(), String> {
        let id = RecordId::from_string(record_id).map_err(|e| e.to_string())?;
        let guard = self.engine.lock().map_err(|e| e.to_string())?;
        guard
            .upsert_vector(&id, embedding)
            .map_err(|e| e.to_string())
    }
}

fn daemon_lock_path(ws: &Workspace) -> Result<PathBuf, CliError> {
    Ok(ws.runtime_dir()?.join("embedd.lock"))
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
    /// One-word status label.
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
