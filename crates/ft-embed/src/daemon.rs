//! Unix-socket embedding daemon (M3 + e7z follow-up).
//!
//! Surface used by `ft-cli`:
//!
//! - [`status`] — probe a socket and return [`DaemonStatus`].
//! - [`send_embed`] — send one embed request, await the response.
//! - [`send_shutdown`] — request a graceful shutdown.
//! - [`serve`] / [`serve_with`] — accept connections, dispatch them to an
//!   [`EmbedService`], honour an exclusive-per-repo file lock, idle timeout,
//!   and an explicit shutdown frame.
//!
//! ## Wire format
//!
//! Each request and each response is a length-prefixed JSON object:
//!
//! ```text
//! +--------+-------------------------+
//! | u32 BE | UTF-8 JSON payload      |
//! | length |                         |
//! +--------+-------------------------+
//! ```
//!
//! Request shape: [`EmbedRequest`]. Response shape: [`EmbedResponse`].
//!
//! ## Still deferred (post-e7z)
//!
//! - In-flight queue, throttling, batching (firetrail-0nu)
//! - Async I/O (tokio); current accept loop is blocking std with non-blocking
//!   poll for idle detection
//! - Multi-tenant request routing (multiple worktrees, same daemon)

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::embedder::Embedder;
use crate::error::EmbedError;
use crate::service::EmbedService;

/// Maximum permitted frame size (1 MiB). Defensive cap against rogue clients.
pub const MAX_FRAME_LEN: u32 = 1024 * 1024;

/// Default idle timeout: a daemon that sees no traffic for this long exits.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Polling cadence for the non-blocking accept loop. Small enough that
/// shutdown / idle detection feels prompt, large enough not to busy-loop.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Daemon liveness states reported by [`status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonStatus {
    /// Socket exists and responded to a ping.
    Running,
    /// Socket does not exist on disk.
    Stopped,
    /// Socket exists but does not accept connections (stale).
    Unreachable,
}

/// One inbound embed request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EmbedRequest {
    /// Liveness probe used by [`status`].
    Ping,
    /// Embed a single text blob.
    Embed {
        /// Raw text to embed; will be hashed and looked up in the cache.
        text: String,
    },
    /// Advisory shutdown request — serve loop exits after replying.
    Shutdown,
}

/// One outbound response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum EmbedResponse {
    /// Reply to [`EmbedRequest::Ping`].
    Pong,
    /// Successful embedding.
    Ok {
        /// Embedding vector.
        embedding: Vec<f32>,
        /// `model_id` of the embedder that produced it.
        model_id: String,
    },
    /// Error returned by the daemon, stringified.
    Err {
        /// Human-readable error message.
        message: String,
    },
    /// Acknowledgement that a [`EmbedRequest::Shutdown`] was accepted.
    ShuttingDown,
}

/// Tunables for [`serve_with`]. Defaults match [`serve`].
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// Exit the serve loop after this much wall-clock idle time. `None`
    /// disables the idle exit.
    pub idle_timeout: Option<Duration>,
    /// Path to the exclusive-per-repo lock file. If `None`, no lock is
    /// acquired (suitable for tests / one-shot scenarios).
    pub lock_path: Option<PathBuf>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            idle_timeout: Some(DEFAULT_IDLE_TIMEOUT),
            lock_path: None,
        }
    }
}

/// Holds an exclusive advisory lock for the daemon's lifetime.
///
/// Dropping the handle releases the lock. The lock file itself is left on
/// disk; readers should treat its presence as informational only.
#[derive(Debug)]
pub struct DaemonLock {
    file: std::fs::File,
    path: PathBuf,
}

impl DaemonLock {
    /// Acquire an exclusive lock at `path`. Fails fast (no blocking) if
    /// another process already holds it.
    pub fn acquire(path: &Path) -> Result<Self, EmbedError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock_exclusive().map_err(|e| {
            EmbedError::Protocol(format!(
                "another ft-embed daemon already holds {}: {e}",
                path.display()
            ))
        })?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Lock file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        // Best effort — if unlock fails we are tearing the process down
        // anyway and the kernel will drop the advisory lock on close.
        let _ = FileExt::unlock(&self.file);
    }
}

/// Probe a socket path. Returns [`DaemonStatus::Stopped`] if the path does
/// not exist, [`DaemonStatus::Running`] if a ping round-trips, otherwise
/// [`DaemonStatus::Unreachable`].
#[must_use]
pub fn status(socket_path: &Path) -> DaemonStatus {
    if !socket_path.exists() {
        return DaemonStatus::Stopped;
    }
    match UnixStream::connect(socket_path) {
        Ok(mut s) => {
            if write_frame(&mut s, &EmbedRequest::Ping).is_err() {
                return DaemonStatus::Unreachable;
            }
            match read_frame::<EmbedResponse>(&mut s) {
                Ok(EmbedResponse::Pong) => DaemonStatus::Running,
                _ => DaemonStatus::Unreachable,
            }
        }
        Err(_) => DaemonStatus::Unreachable,
    }
}

/// Send one embed request, await one response.
pub fn send_embed(socket_path: &Path, text: &str) -> Result<Vec<f32>, EmbedError> {
    let mut s = UnixStream::connect(socket_path)?;
    write_frame(
        &mut s,
        &EmbedRequest::Embed {
            text: text.to_string(),
        },
    )?;
    match read_frame::<EmbedResponse>(&mut s)? {
        EmbedResponse::Ok { embedding, .. } => Ok(embedding),
        EmbedResponse::Err { message } => Err(EmbedError::Inference(message)),
        EmbedResponse::Pong | EmbedResponse::ShuttingDown => Err(EmbedError::Protocol(
            "expected Ok or Err, got control frame".to_string(),
        )),
    }
}

/// Ask the daemon to shut down. Returns once it has acknowledged.
pub fn send_shutdown(socket_path: &Path) -> Result<(), EmbedError> {
    let mut s = UnixStream::connect(socket_path)?;
    write_frame(&mut s, &EmbedRequest::Shutdown)?;
    match read_frame::<EmbedResponse>(&mut s)? {
        EmbedResponse::ShuttingDown => Ok(()),
        EmbedResponse::Err { message } => Err(EmbedError::Inference(message)),
        _ => Err(EmbedError::Protocol(
            "expected ShuttingDown ack".to_string(),
        )),
    }
}

/// Accept connections on `socket_path`, dispatch each request to `service`.
///
/// Uses [`ServeOptions::default`] (default idle timeout, no lock). Prefer
/// [`serve_with`] when you need the per-repo lock or a custom idle timeout.
pub fn serve<E: Embedder>(socket_path: &Path, service: &EmbedService<E>) -> Result<(), EmbedError> {
    serve_with(socket_path, service, &ServeOptions::default())
}

/// Like [`serve`] but with explicit options.
///
/// When `opts.lock_path` is set, an exclusive advisory lock is acquired
/// before binding; a second daemon for the same repo fails fast. The lock
/// is released when the serve loop exits.
pub fn serve_with<E: Embedder>(
    socket_path: &Path,
    service: &EmbedService<E>,
    opts: &ServeOptions,
) -> Result<(), EmbedError> {
    let _lock = match opts.lock_path.as_deref() {
        Some(p) => Some(DaemonLock::acquire(p)?),
        None => None,
    };

    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    listener.set_nonblocking(true)?;
    tracing::info!(socket = %socket_path.display(), "ft-embed daemon listening");

    let mut last_activity = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _addr)) => {
                last_activity = Instant::now();
                // Per-connection I/O is blocking — clear non-blocking flag
                // we inherited from the listener.
                let _ = stream.set_nonblocking(false);
                match handle_connection(&mut stream, service) {
                    Ok(ConnectionOutcome::Continue) => {}
                    Ok(ConnectionOutcome::Shutdown) => {
                        tracing::info!("ft-embed daemon shutting down (client request)");
                        break;
                    }
                    Err(e) => tracing::warn!(error = %e, "connection handler failed"),
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(timeout) = opts.idle_timeout {
                    if last_activity.elapsed() >= timeout {
                        tracing::info!(
                            timeout_secs = timeout.as_secs(),
                            "ft-embed daemon shutting down (idle)"
                        );
                        break;
                    }
                }
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(e) => {
                tracing::warn!(error = %e, "accept failed");
                // Transient accept error — back off briefly and continue.
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
        }
    }

    // Tidy up the socket file so a subsequent `status()` sees `Stopped`.
    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ConnectionOutcome {
    Continue,
    Shutdown,
}

/// Handle one client connection: read one request, write one response.
fn handle_connection<E: Embedder>(
    stream: &mut UnixStream,
    service: &EmbedService<E>,
) -> Result<ConnectionOutcome, EmbedError> {
    let req: EmbedRequest = read_frame(stream)?;
    let (resp, outcome) = match req {
        EmbedRequest::Ping => (EmbedResponse::Pong, ConnectionOutcome::Continue),
        EmbedRequest::Embed { text } => {
            let resp = match service.embed_text(&text) {
                Ok(embedding) => EmbedResponse::Ok {
                    embedding,
                    model_id: service.embedder().model_id().to_string(),
                },
                Err(e) => EmbedResponse::Err {
                    message: e.to_string(),
                },
            };
            (resp, ConnectionOutcome::Continue)
        }
        EmbedRequest::Shutdown => (EmbedResponse::ShuttingDown, ConnectionOutcome::Shutdown),
    };
    write_frame(stream, &resp)?;
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

fn write_frame<W: Write, T: Serialize>(w: &mut W, value: &T) -> Result<(), EmbedError> {
    let payload = serde_json::to_vec(value).map_err(|e| EmbedError::Protocol(e.to_string()))?;
    let len = u32::try_from(payload.len())
        .map_err(|_| EmbedError::Protocol("frame too large".to_string()))?;
    if len > MAX_FRAME_LEN {
        return Err(EmbedError::Protocol(format!(
            "frame {len} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}"
        )));
    }
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&payload)?;
    w.flush()?;
    Ok(())
}

fn read_frame<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> Result<T, EmbedError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(EmbedError::Protocol(format!(
            "frame {len} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}"
        )));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(|e| EmbedError::Protocol(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use tempfile::tempdir;

    use super::*;
    use crate::cache::EmbeddingCache;
    use crate::embedder::MockEmbedder;

    fn build_service(tmp: &Path) -> EmbedService<MockEmbedder> {
        let cache = EmbeddingCache::open(tmp.join("cache.db")).unwrap();
        EmbedService::new(MockEmbedder::new(123, 16), cache)
    }

    fn wait_for(predicate: impl Fn() -> bool, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if predicate() {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        predicate()
    }

    #[test]
    fn status_stopped_when_socket_missing() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("missing.sock");
        assert_eq!(status(&sock), DaemonStatus::Stopped);
    }

    #[test]
    fn serve_then_send_embed_round_trip_and_shutdown() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("ft-embed.sock");
        let svc_dir = dir.path().to_path_buf();
        let sock_for_server = sock.clone();

        let server = thread::spawn(move || {
            let svc = build_service(&svc_dir);
            serve_with(
                &sock_for_server,
                &svc,
                &ServeOptions {
                    idle_timeout: Some(Duration::from_secs(5)),
                    lock_path: None,
                },
            )
            .unwrap();
        });

        assert!(wait_for(|| status(&sock) == DaemonStatus::Running, Duration::from_secs(2)));
        let v = send_embed(&sock, "hello daemon").unwrap();
        assert_eq!(v.len(), 16);

        send_shutdown(&sock).unwrap();
        server.join().unwrap();
        assert!(!sock.exists(), "socket file should be removed on shutdown");
    }

    #[test]
    fn idle_timeout_exits_serve_loop() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("idle.sock");
        let svc_dir = dir.path().to_path_buf();
        let sock_for_server = sock.clone();

        let started = thread::spawn(move || {
            let svc = build_service(&svc_dir);
            let start = Instant::now();
            serve_with(
                &sock_for_server,
                &svc,
                &ServeOptions {
                    idle_timeout: Some(Duration::from_millis(150)),
                    lock_path: None,
                },
            )
            .unwrap();
            start.elapsed()
        });

        let elapsed = started.join().unwrap();
        assert!(
            elapsed >= Duration::from_millis(150) && elapsed < Duration::from_secs(3),
            "idle exit should occur near the 150ms timeout, got {elapsed:?}"
        );
        assert!(!sock.exists());
    }

    #[test]
    fn lock_blocks_second_daemon() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("embedd.lock");
        let held = DaemonLock::acquire(&lock).unwrap();

        let sock = dir.path().join("locked.sock");
        let svc = build_service(dir.path());
        let err = serve_with(
            &sock,
            &svc,
            &ServeOptions {
                idle_timeout: Some(Duration::from_millis(50)),
                lock_path: Some(lock.clone()),
            },
        )
        .unwrap_err();
        match err {
            EmbedError::Protocol(msg) => assert!(msg.contains("already holds")),
            other => panic!("expected Protocol error, got {other:?}"),
        }

        drop(held);
        // After releasing, a fresh daemon can acquire the lock.
        let stop_flag = Arc::new(AtomicBool::new(false));
        let svc_dir = dir.path().to_path_buf();
        let sock2 = sock.clone();
        let lock2 = lock.clone();
        let flag2 = stop_flag.clone();
        let h = thread::spawn(move || {
            let svc = build_service(&svc_dir);
            // Short idle timeout so the test exits even if shutdown frame fails.
            let _ = serve_with(
                &sock2,
                &svc,
                &ServeOptions {
                    idle_timeout: Some(Duration::from_secs(3)),
                    lock_path: Some(lock2),
                },
            );
            flag2.store(true, Ordering::SeqCst);
        });
        assert!(wait_for(|| status(&sock) == DaemonStatus::Running, Duration::from_secs(2)));
        send_shutdown(&sock).unwrap();
        h.join().unwrap();
        assert!(stop_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn oversized_frame_rejected_on_write() {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut writer = a;
        let mut reader = b;
        writer
            .write_all(&(MAX_FRAME_LEN + 1).to_be_bytes())
            .unwrap();
        writer.flush().unwrap();
        let err = read_frame::<EmbedRequest>(&mut reader).unwrap_err();
        assert!(matches!(err, EmbedError::Protocol(_)));
    }
}
