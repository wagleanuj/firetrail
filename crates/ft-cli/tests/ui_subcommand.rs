//! Integration tests for the `firetrail ui` subcommand.
//!
//! The subcommand spawns the sibling `ft-ui` binary. Cargo places both
//! binaries side-by-side in `target/<profile>/`, so `current_exe().parent()`
//! discovery works inside the integration test.
//!
//! We always exercise `--foreground` so the test owns the lifetime of both
//! the `firetrail` parent and the `ft-ui` grandchild.

#![cfg(unix)]
#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ft_testkit::TestRepo;

fn firetrail_init(root: &Path) {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let status = Command::new(bin)
        .args(["init", "--json"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn firetrail init");
    assert!(status.success(), "firetrail init must succeed");
}

fn spawn_firetrail_ui(root: &Path, extra: &[&str]) -> Child {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    Command::new(bin)
        .arg("ui")
        .arg("--no-open")
        .arg("--foreground")
        .args(extra)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn firetrail ui")
}

/// Read stdout lines until we see the canonical `firetrail-ui ready:`
/// announcement, or `deadline` elapses.
fn wait_for_ready_url(child: &mut Child, deadline: Duration) -> Result<String, String> {
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
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

/// Send SIGKILL to the parent (the strongest signal we can deliver without
/// pulling in `unsafe`/`libc`) and wait for it to reap within `timeout`.
fn terminate(child: &mut Child, timeout: Duration) -> bool {
    let _ = child.kill();
    let started = Instant::now();
    while started.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn test_ui_no_open_foreground() {
    let tr = TestRepo::new().expect("test repo");
    firetrail_init(tr.root());

    let mut child = spawn_firetrail_ui(tr.root(), &[]);
    let url = match wait_for_ready_url(&mut child, Duration::from_secs(10)) {
        Ok(u) => u,
        Err(e) => {
            let mut stderr = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut stderr);
            }
            let _ = child.kill();
            let _ = child.wait();
            panic!("did not see ready line: {e}\nstderr:\n{stderr}");
        }
    };

    assert!(
        url.starts_with("http://127.0.0.1:"),
        "ready URL must be loopback http: {url}"
    );
    assert!(
        url.contains("/?token="),
        "ready URL must include bootstrap token: {url}"
    );

    let clean = terminate(&mut child, Duration::from_secs(5));
    assert!(clean, "firetrail ui parent must exit within 5s of signal");
}

#[test]
fn test_ui_emits_url_then_keepalive() {
    let tr = TestRepo::new().expect("test repo");
    firetrail_init(tr.root());

    let mut child = spawn_firetrail_ui(tr.root(), &[]);
    let url =
        wait_for_ready_url(&mut child, Duration::from_secs(10)).expect("ready line within 10s");
    assert!(url.starts_with("http://127.0.0.1:"), "got: {url}");

    // Confirm the server is actually listening — a TCP connect is enough
    // here; we don't take a reqwest dep just for liveness.
    let host_port = url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let addr: std::net::SocketAddr = host_port.parse().expect("socket addr");
    let stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2));
    assert!(
        stream.is_ok(),
        "could not connect to {host_port}: {stream:?}"
    );
    drop(stream);

    let clean = terminate(&mut child, Duration::from_secs(5));
    assert!(clean, "firetrail ui parent must exit within 5s of signal");
}
