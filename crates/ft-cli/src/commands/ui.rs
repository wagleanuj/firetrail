//! `firetrail ui` — spawn the `ft-ui` HTTP/JSON server and open the browser.
//!
//! Behaviour:
//!
//! - Resolves the workspace (`.firetrail/config.yml` must be present).
//! - Locates the sibling `ft-ui` binary next to the running `firetrail`
//!   executable, falling back to `$PATH`.
//! - Spawns `ft-ui --workspace <root> --bind <addr> [--dev] [--foreground]`
//!   with a piped stdout, and reads lines until it sees the canonical
//!   `firetrail-ui ready: <url>` announcement (10s timeout).
//! - Opens the URL in the user's browser unless `--no-open` was supplied.
//! - In `--foreground` mode the parent stays attached, forwarding the
//!   child's subsequent stdout to the user, and waits for it to exit.
//! - In background mode (Unix) the child is detached via `setsid(2)` +
//!   `/dev/null` stdio and the parent prints a one-line success message.
//! - Non-Unix without `--foreground` returns a clear user error.
//!
//! The `--no-open` flag is **not** forwarded to `ft-ui`; the CLI owns
//! browser-opening so the server can stay UI-toolchain-agnostic.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::cli::{GlobalOpts, UiArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const CMD_UI: &str = "ui";
const DEFAULT_BIND: &str = "127.0.0.1:0";
const READY_PREFIX: &str = "firetrail-ui ready: ";
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// `firetrail ui` entry point.
pub fn run(args: &UiArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_UI, global.workspace.as_deref())?;
    let ft_ui = locate_ft_ui()?;

    let bind = resolve_bind(args);

    let workspace_arg = ws.root.display().to_string();
    let mut cmd = Command::new(&ft_ui);
    cmd.arg("--workspace")
        .arg(&workspace_arg)
        .arg("--bind")
        .arg(&bind);
    if args.dev {
        cmd.arg("--dev");
    }
    if args.foreground {
        cmd.arg("--foreground");
    }

    if args.foreground || cfg!(not(unix)) {
        spawn_foreground(cmd, args)
    } else {
        #[cfg(unix)]
        {
            spawn_background(cmd, args)
        }
        #[cfg(not(unix))]
        {
            Err(CliError::user(
                CMD_UI,
                "background ft-ui spawn is Unix-only; rerun with --foreground",
            ))
        }
    }
}

/// Resolve `--bind` / `--port` into the address forwarded to `ft-ui`.
fn resolve_bind(args: &UiArgs) -> String {
    if let Some(b) = args.bind.as_deref() {
        if let Some(port) = args.port {
            // Replace the port on the supplied bind address. If parsing
            // fails, fall back to the literal string the user gave us
            // (ft-ui will surface the parse error itself).
            if let Ok(mut sa) = b.parse::<std::net::SocketAddr>() {
                sa.set_port(port);
                return sa.to_string();
            }
        }
        return b.to_string();
    }
    if let Some(port) = args.port {
        return format!("127.0.0.1:{port}");
    }
    DEFAULT_BIND.to_string()
}

/// Find the sibling `ft-ui` binary. Strategy:
/// 1. Look next to `current_exe()`.
/// 2. Fall back to walking `$PATH`.
fn locate_ft_ui() -> Result<PathBuf, CliError> {
    let bin_name = if cfg!(windows) { "ft-ui.exe" } else { "ft-ui" };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join(bin_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    if let Some(path) = path_lookup(bin_name) {
        return Ok(path);
    }

    Err(CliError::user(
        CMD_UI,
        format!(
            "could not find the `{bin_name}` binary next to firetrail or on $PATH.\n\
             Build it with `cargo build -p ft-ui` (debug) or install it alongside `firetrail`."
        ),
    ))
}

/// Tiny `$PATH` walker so we avoid pulling in the `which` crate.
fn path_lookup(bin_name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn spawn_foreground(mut cmd: Command, args: &UiArgs) -> Result<CommandOutcome, CliError> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .map_err(|e| CliError::internal(CMD_UI, format!("spawn ft-ui: {e}")))?;
    let pid = child.id();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::internal(CMD_UI, "ft-ui stdout was not piped"))?;
    let url = match read_ready_url(stdout) {
        Ok((url, reader)) => {
            // Drain remaining stdout lines on a background thread so any
            // tracing JSON the user has enabled still reaches their tty.
            thread::spawn(move || forward_stdout(reader));
            url
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };

    // Re-emit the canonical ready line on the parent's stdout so callers
    // scripting against `firetrail ui` get the same single-line contract
    // as `ft-ui` itself.
    println!("firetrail-ui ready: {url}");

    if !args.no_open {
        open_browser(&url);
    }

    eprintln!("firetrail ui running at {url} (pid {pid}) — Ctrl-C to stop.");

    let status = child
        .wait()
        .map_err(|e| CliError::internal(CMD_UI, format!("wait ft-ui: {e}")))?;
    if !status.success() {
        return Err(CliError::internal(
            CMD_UI,
            format!("ft-ui exited with status {status}"),
        ));
    }

    Ok(CommandOutcome::Ui(UiOutcome {
        url,
        pid,
        mode: "foreground",
        warnings: Vec::new(),
    }))
}

#[cfg(unix)]
fn spawn_background(mut cmd: Command, args: &UiArgs) -> Result<CommandOutcome, CliError> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    // We mirror `daemon_cmd::spawn_background` here: no `setsid(2)` call
    // (which would require `unsafe` and this crate forbids it). The child
    // stays in our process group, which is acceptable because:
    //   * The embedded heartbeat watchdog terminates the server after the
    //     SPA stops pinging.
    //   * Terminal SIGINT/SIGTERM during `firetrail ui` is the documented
    //     stop signal; both parent and child handle it cleanly.
    // The child is detached by forgetting the `Child` handle and dropping
    // its inherited pipe FDs once we've captured the ready URL.

    let mut child = cmd
        .spawn()
        .map_err(|e| CliError::internal(CMD_UI, format!("spawn ft-ui: {e}")))?;
    let pid = child.id();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::internal(CMD_UI, "ft-ui stdout was not piped"))?;
    let (url, reader) = match read_ready_url(stdout) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };

    println!("firetrail-ui ready: {url}");

    if !args.no_open {
        open_browser(&url);
    }

    // Detach: forget the child handle and drop the BufReader so its file
    // descriptor closes; the server has already moved past the ready line.
    drop(reader);
    std::mem::forget(child);

    eprintln!(
        "firetrail ui running at {url} (pid {pid}). \
         Close the browser tab to stop — the server idles out after 60s."
    );

    Ok(CommandOutcome::Ui(UiOutcome {
        url,
        pid,
        mode: "background",
        warnings: Vec::new(),
    }))
}

/// Read piped stdout until either the `firetrail-ui ready:` line appears
/// or [`READY_TIMEOUT`] elapses.
fn read_ready_url(
    stdout: std::process::ChildStdout,
) -> Result<(String, BufReader<std::process::ChildStdout>), CliError> {
    let (tx, rx) = mpsc::channel::<std::io::Result<String>>();
    let mut reader = BufReader::new(stdout);

    // Read on a worker thread so we can enforce a wall-clock timeout.
    // We move the reader in, then move it back out via a channel after
    // the ready line so the caller can keep streaming subsequent output.
    let (reader_tx, reader_rx) = mpsc::channel::<BufReader<std::process::ChildStdout>>();
    thread::spawn(move || {
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "ft-ui closed stdout before announcing readiness",
                    )));
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                    if let Some(rest) = trimmed.strip_prefix(READY_PREFIX) {
                        let url = rest.split_whitespace().next().unwrap_or("").to_string();
                        if tx.send(Ok(url)).is_ok() {
                            // Hand the reader back to the caller so it can
                            // continue forwarding output if it wants to.
                            let _ = reader_tx.send(reader);
                        }
                        return;
                    }
                    // Non-ready lines before readiness are echoed to stderr
                    // so the user sees any structured tracing output that
                    // leaks onto stdout.
                    eprintln!("[ft-ui] {trimmed}");
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
    });

    let url = match rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(url)) => url,
        Ok(Err(e)) => {
            return Err(CliError::internal(CMD_UI, format!("read ft-ui stdout: {e}")));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(CliError::internal(
                CMD_UI,
                format!(
                    "ft-ui did not announce readiness within {}s",
                    READY_TIMEOUT.as_secs()
                ),
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(CliError::internal(
                CMD_UI,
                "ft-ui worker thread terminated unexpectedly",
            ));
        }
    };

    let reader = reader_rx.recv().map_err(|_| {
        CliError::internal(CMD_UI, "ft-ui reader channel closed unexpectedly")
    })?;
    Ok((url, reader))
}

/// Echo subsequent ft-ui stdout lines to the user.
fn forward_stdout(reader: BufReader<std::process::ChildStdout>) {
    for line in reader.lines().map_while(Result::ok) {
        eprintln!("[ft-ui] {line}");
    }
}

/// Best-effort browser opener. Failures are swallowed (with a stderr note)
/// because not having a browser is not a hard error — the user can still
/// copy the URL.
fn open_browser(url: &str) {
    if let Err(e) = open::that(url) {
        eprintln!("note: could not open browser ({e}); visit {url} manually");
    }
}

/// JSON / markdown view of the `firetrail ui` command result.
#[derive(Debug, Clone, Serialize)]
pub struct UiOutcome {
    /// URL announced by `ft-ui` on stdout.
    pub url: String,
    /// Child PID.
    pub pid: u32,
    /// `"foreground"` or `"background"`.
    pub mode: &'static str,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl UiOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        format!(
            "**ui** url=`{}` pid={} mode={}\n",
            self.url, self.pid, self.mode
        )
    }
    /// One-line quiet summary.
    pub fn quiet_line(&self) -> String {
        format!("ui: {} (pid {})", self.url, self.pid)
    }
}
