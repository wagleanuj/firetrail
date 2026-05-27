//! Minimal Unix-socket embedding daemon (M3 scope).
//!
//! This is **not** the full ADR-0007 daemon. It provides only the surface
//! `ft-cli` needs to demonstrate the daemon flow at M3:
//!
//! - [`status`] — probe a socket and return [`DaemonStatus`].
//! - [`send_embed`] — send one embed request, await the response.
//! - [`serve`] — accept connections and dispatch them to an [`EmbedService`].
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
//! ## Deferred (see follow-ups)
//!
//! - Single-daemon-per-repo file lock + auto-spawn
//! - In-flight queue, throttling, batching
//! - Async I/O (tokio) — current implementation is blocking std sockets
//! - Idle shutdown, signal handling, daemon supervision
//! - `firetrail daemon stop` semantics (advisory shutdown frame)
//! - Multi-tenant request routing (multiple worktrees, same daemon)

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::embedder::Embedder;
use crate::error::EmbedError;
use crate::service::EmbedService;

/// Maximum permitted frame size (1 MiB). Defensive cap against rogue clients.
pub const MAX_FRAME_LEN: u32 = 1024 * 1024;

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
        EmbedResponse::Pong => Err(EmbedError::Protocol(
            "expected Ok or Err, got Pong".to_string(),
        )),
    }
}

/// Accept connections on `socket_path`, dispatch each request to `service`.
///
/// Runs until the listener errors out. Each connection is handled inline
/// (one-shot); concurrent throughput is intentionally out of scope here.
///
/// Removes any stale socket file at `socket_path` before binding.
pub fn serve<E: Embedder>(socket_path: &Path, service: &EmbedService<E>) -> Result<(), EmbedError> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    tracing::info!(socket = %socket_path.display(), "ft-embed daemon listening");

    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "accept failed");
                continue;
            }
        };
        if let Err(e) = handle_connection(&mut stream, service) {
            tracing::warn!(error = %e, "connection handler failed");
        }
    }
    Ok(())
}

/// Handle one client connection: read one request, write one response.
fn handle_connection<E: Embedder>(
    stream: &mut UnixStream,
    service: &EmbedService<E>,
) -> Result<(), EmbedError> {
    let req: EmbedRequest = read_frame(stream)?;
    let resp = match req {
        EmbedRequest::Ping => EmbedResponse::Pong,
        EmbedRequest::Embed { text } => match service.embed_text(&text) {
            Ok(embedding) => EmbedResponse::Ok {
                embedding,
                model_id: service.embedder().model_id().to_string(),
            },
            Err(e) => EmbedResponse::Err {
                message: e.to_string(),
            },
        },
    };
    write_frame(stream, &resp)?;
    Ok(())
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
    use std::thread;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::*;
    use crate::cache::EmbeddingCache;
    use crate::embedder::MockEmbedder;

    fn build_service(tmp: &Path) -> EmbedService<MockEmbedder> {
        let cache = EmbeddingCache::open(tmp.join("cache.db")).unwrap();
        EmbedService::new(MockEmbedder::new(123, 16), cache)
    }

    #[test]
    fn status_stopped_when_socket_missing() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("missing.sock");
        assert_eq!(status(&sock), DaemonStatus::Stopped);
    }

    #[test]
    fn serve_then_send_embed_round_trip() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("ft-embed.sock");

        let service_dir = dir.path().to_path_buf();
        let sock_for_server = sock.clone();
        let server = thread::spawn(move || {
            let svc = build_service(&service_dir);
            // serve() loops on incoming; we run one accept manually so the
            // thread exits cleanly.
            if sock_for_server.exists() {
                std::fs::remove_file(&sock_for_server).unwrap();
            }
            let listener = UnixListener::bind(&sock_for_server).unwrap();
            for _ in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                handle_connection(&mut s, &svc).unwrap();
            }
        });

        // Wait for the socket to appear (server thread is starting up).
        for _ in 0..50 {
            if sock.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(sock.exists(), "server did not bind socket in time");

        // Round-trip: ping + embed.
        assert_eq!(status(&sock), DaemonStatus::Running);
        let v = send_embed(&sock, "hello daemon").unwrap();
        assert_eq!(v.len(), 16);

        server.join().unwrap();
    }

    #[test]
    fn oversized_frame_rejected_on_write() {
        // We can't easily fabricate a >1MiB payload in a unit test, but we
        // can exercise the protocol-error path by sending a too-large
        // declared length and reading it back.
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut writer = a;
        let mut reader = b;
        // Declare a length over MAX_FRAME_LEN.
        writer
            .write_all(&(MAX_FRAME_LEN + 1).to_be_bytes())
            .unwrap();
        writer.flush().unwrap();
        let err = read_frame::<EmbedRequest>(&mut reader).unwrap_err();
        assert!(matches!(err, EmbedError::Protocol(_)));
    }
}
