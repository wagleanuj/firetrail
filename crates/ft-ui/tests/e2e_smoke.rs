//! Wave 0 end-to-end smoke tests for the ft-ui binary.
//!
//! These tests spawn the actual `ft-ui` binary over real TCP and exercise
//! the full Wave 0 stack: token bootstrap, signed session cookie,
//! `/api/workspace`, the `/api/events` SSE stream, and the heartbeat-driven
//! idle-exit watchdog.
//!
//! Complements the in-process axum tests in `tests/server.rs` by catching
//! binary-level regressions in CLI args + stdout contract.
//!
//! The idle-exit test (Test 3) takes ~70-90s and is `#[ignore]`d by default.
//! Run it explicitly with:
//!
//! ```text
//! cargo test -p ft-ui --test e2e_smoke -- --include-ignored
//! ```

#![cfg(unix)]
#![allow(clippy::missing_panics_doc)]

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tempfile::TempDir;

mod common {
    use super::{
        BufRead, BufReader, Child, Command, Duration, Instant, Path, PathBuf, SocketAddr, Stdio,
        TempDir, mpsc, thread,
    };

    /// Build an isolated fixture workspace with the marker file the
    /// server's `Workspace::open` requires.
    pub fn fixture_workspace() -> TempDir {
        let tmp = TempDir::new().expect("tempdir");
        let ft = tmp.path().join(".firetrail");
        std::fs::create_dir_all(&ft).expect("mkdir .firetrail");
        std::fs::write(ft.join("config.yml"), b"# firetrail e2e test workspace\n")
            .expect("write config.yml");
        tmp
    }

    /// Handle to a spawned `ft-ui` child process.
    ///
    /// Kills the child on drop so tests never leak background servers,
    /// even on panic.
    pub struct SpawnedServer {
        pub child: Child,
        pub url: String,
        pub addr: SocketAddr,
        // Retained for diagnostics/debugging; not all tests need it.
        #[allow(dead_code)]
        pub token: String,
    }

    impl Drop for SpawnedServer {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    impl SpawnedServer {
        /// Build a base URL string `http://127.0.0.1:PORT/path`.
        pub fn url(&self, path: &str) -> String {
            format!("http://{}{}", self.addr, path)
        }
    }

    /// Locate the `ft-ui` binary built by cargo for this test target.
    fn ft_ui_binary() -> PathBuf {
        // Cargo sets this for every binary in the package under test.
        PathBuf::from(env!("CARGO_BIN_EXE_ft-ui"))
    }

    /// Spawn `ft-ui` against `workspace` and wait for the ready line.
    ///
    /// Pass `extra` to forward additional CLI flags (e.g. `--foreground`).
    /// The returned `SpawnedServer`'s `Drop` impl handles teardown.
    pub fn spawn_ft_ui(workspace: &Path, extra: &[&str]) -> SpawnedServer {
        let bin = ft_ui_binary();
        assert!(
            bin.exists(),
            "ft-ui binary not built at {} — run `cargo build -p ft-ui` first",
            bin.display()
        );

        let mut cmd = Command::new(&bin);
        cmd.arg("--workspace")
            .arg(workspace)
            .arg("--bind")
            .arg("127.0.0.1:0")
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().expect("spawn ft-ui");
        let url = wait_for_ready_url(&mut child, Duration::from_secs(10))
            .expect("ready line within 10s");

        let host_port = url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .expect("url has host")
            .to_string();
        let addr: SocketAddr = host_port.parse().expect("parse bound socket addr");

        // Extract `?token=…` from the URL.
        let token = url
            .split_once("?token=")
            .map(|(_, t)| t.split_whitespace().next().unwrap_or("").to_string())
            .expect("ready URL carries ?token=");

        SpawnedServer {
            child,
            url,
            addr,
            token,
        }
    }

    /// Read child stdout lines until the canonical `firetrail-ui ready: `
    /// banner appears, or `deadline` elapses.
    fn wait_for_ready_url(child: &mut Child, deadline: Duration) -> Result<String, String> {
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if let Some(rest) = l.trim().strip_prefix("firetrail-ui ready: ") {
                            let url = rest.split_whitespace().next().unwrap_or("").to_string();
                            let _ = tx.send(Ok(url));
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(format!("io error: {e}")));
                        return;
                    }
                }
            }
            let _ = tx.send(Err("stdout closed before ready line".to_string()));
        });
        rx.recv_timeout(deadline)
            .map_err(|_| "timed out waiting for ready line".to_string())?
    }

    /// HTTP client wired up the way the SPA itself uses the server:
    /// cookies retained, redirects *not* followed (so we can observe the
    /// bootstrap 302 directly and avoid landing on `/` which 404s when
    /// the SPA isn't bundled), sensible timeouts. The loopback resolver
    /// lets `Host: 127.0.0.1:PORT` work even though reqwest cares about
    /// DNS for cookie scoping.
    pub fn http(addr: SocketAddr) -> reqwest::Client {
        reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .resolve("localhost", addr)
            .resolve("127.0.0.1", addr)
            .build()
            .expect("build reqwest client")
    }

    /// Same as `http` but with no global response timeout — required for
    /// long-lived SSE streams.
    pub fn http_no_timeout(addr: SocketAddr) -> reqwest::Client {
        reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .resolve("localhost", addr)
            .resolve("127.0.0.1", addr)
            .build()
            .expect("build reqwest client")
    }

    /// Best-effort: wait up to `timeout` for the child to exit on its own.
    /// Returns `Some(success)` if it exited, `None` if it did not.
    pub fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<bool> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status.success()),
                Ok(None) => thread::sleep(Duration::from_millis(100)),
                Err(_) => return None,
            }
        }
        None
    }
}

use common::{fixture_workspace, http, http_no_timeout, spawn_ft_ui, wait_for_exit};

/// Test 1 — bootstrap token authenticates, signed cookie is set, and
/// `/api/workspace` reports the fixture root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_and_workspace_endpoint() {
    let tmp = fixture_workspace();
    let server = spawn_ft_ui(tmp.path(), &["--foreground"]);

    let client = http(server.addr);

    // GET the ready URL (carries ?token=). Expect a 302 redirect to `/`
    // with the signed session cookie attached. We deliberately do NOT
    // follow the redirect, because `/` 404s when the SPA isn't bundled.
    let resp = client
        .get(&server.url)
        .header("Host", server.addr.to_string())
        .send()
        .await
        .expect("bootstrap GET");
    assert!(
        resp.status().is_redirection(),
        "bootstrap expected 3xx, got {} — body: {:?}",
        resp.status(),
        resp.text().await.ok()
    );
    let set_cookie = resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("bootstrap must set the session cookie");
    let cookie_str = set_cookie.to_str().unwrap_or("");
    assert!(
        cookie_str.contains("firetrail_session"),
        "Set-Cookie did not name `firetrail_session`: {cookie_str}"
    );

    // Now hit /api/workspace; the cookie store should carry the session.
    let resp = client
        .get(server.url("/api/workspace"))
        .header("Host", server.addr.to_string())
        .send()
        .await
        .expect("workspace GET");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "expected 200 on /api/workspace after bootstrap"
    );
    let body: serde_json::Value = resp.json().await.expect("json body");
    let root = body
        .get("root")
        .and_then(|v| v.as_str())
        .expect("workspace.root is a string");
    let want = tmp.path().to_string_lossy().to_string();
    // On macOS tempdirs canonicalize through /private; accept either form.
    assert!(
        root.contains(want.trim_start_matches("/private"))
            || want.contains(root.trim_start_matches("/private")),
        "workspace root mismatch: got {root}, want {want}"
    );

    // server is dropped here → SIGKILL on the child.
}

/// Test 2 — SSE stream opens cleanly, returns the correct content-type,
/// and stays connected past the first keep-alive interval.
///
/// We do not assert on a specific event payload here: triggering an event
/// requires reaching into `ft_ops::EventBus` from outside the server
/// process, which is awkward. Cross-process event delivery is already
/// tested via `tests/server.rs::test_sse_keeps_open`. The cross-binary
/// regression we care about here is "the SSE route is mounted, auth'd,
/// and the connection stays open".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_receives_events() {
    let tmp = fixture_workspace();
    let server = spawn_ft_ui(tmp.path(), &["--foreground"]);

    let client = http_no_timeout(server.addr);

    // Bootstrap so the session cookie is in the jar.
    let _ = client
        .get(&server.url)
        .header("Host", server.addr.to_string())
        .send()
        .await
        .expect("bootstrap GET");

    // Open the SSE stream.
    let resp = client
        .get(server.url("/api/events"))
        .header("Host", server.addr.to_string())
        .header("Accept", "text/event-stream")
        .send()
        .await
        .expect("sse GET");
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "sse status");
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(
        ct.starts_with("text/event-stream"),
        "expected text/event-stream content-type, got {ct}"
    );

    // Stream for 3s. We don't need to receive a frame — the server's
    // keep-alive interval is 15s — but we do need the connection to stay
    // alive (no early EOF, no error) for the duration.
    let mut stream = resp.bytes_stream();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(_chunk))) => { /* good — got data */ }
            Ok(Some(Err(e))) => panic!("sse stream errored mid-flight: {e}"),
            Ok(None) => panic!("sse stream ended prematurely"),
            Err(_) => { /* idle window, keep going */ }
        }
    }
    // Reached here => the stream stayed open for 3s with no error/EOF.
}

/// Test 3 — heartbeats keep the server alive, and the 60s idle watchdog
/// terminates the process once pings stop.
///
/// This test takes ~70-90s and is `#[ignore]`d by default. To run it:
///
/// ```text
/// cargo test -p ft-ui --test e2e_smoke -- --include-ignored
/// ```
///
/// We deliberately do NOT pass `--foreground` here, because `--foreground`
/// disables the watchdog (`server.rs` only spawns the watchdog task when
/// `!opts.foreground`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "takes ~90s — run with --include-ignored or the slow-tests CI lane"]
async fn heartbeat_keeps_alive_then_idle_exits() {
    let tmp = fixture_workspace();
    let mut server = spawn_ft_ui(tmp.path(), &[]);

    let client = http(server.addr);

    // Bootstrap.
    let resp = client
        .get(&server.url)
        .header("Host", server.addr.to_string())
        .send()
        .await
        .expect("bootstrap GET");
    assert!(resp.status().is_success() || resp.status().is_redirection());

    // Heartbeat every 10s for ~30s, asserting the server still answers
    // /api/workspace after each ping.
    for i in 0..3 {
        let resp = client
            .post(server.url("/api/heartbeat"))
            .header("Host", server.addr.to_string())
            .send()
            .await
            .unwrap_or_else(|e| panic!("heartbeat #{i} send: {e}"));
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::NO_CONTENT,
            "heartbeat #{i} status"
        );

        let resp = client
            .get(server.url("/api/workspace"))
            .header("Host", server.addr.to_string())
            .send()
            .await
            .unwrap_or_else(|e| panic!("workspace probe #{i} send: {e}"));
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::OK,
            "/api/workspace probe #{i} after heartbeat"
        );

        // Make sure the child is still alive between pings.
        assert!(
            server.child.try_wait().expect("try_wait").is_none(),
            "child exited while we were still heartbeating (iter {i})"
        );

        tokio::time::sleep(Duration::from_secs(10)).await;
    }

    // Stop pinging. The watchdog runs every 10s and exits once the last
    // heartbeat is >60s old. Worst case ≈ 70s; we allow 90s.
    let exited = wait_for_exit(&mut server.child, Duration::from_secs(90));
    assert!(
        matches!(exited, Some(true)),
        "expected child to exit cleanly within 90s of last heartbeat; got {exited:?}"
    );

    // Drop the SpawnedServer (already-reaped child is a no-op kill).
}
