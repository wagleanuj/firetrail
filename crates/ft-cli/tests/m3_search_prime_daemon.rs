//! Integration tests for the M3 CLI surface: search, similar, prime,
//! index rebuild/refresh, daemon start (foreground) + status.

mod common;

use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::{fresh_repo, id_from_create, parse_json, run_firetrail};
use ft_embed::daemon::{self, DaemonStatus};

fn create_two_records(root: &std::path::Path) -> (String, String) {
    let t1 = run_firetrail(
        root,
        &[
            "task",
            "create",
            "Investigate flaky payment webhook",
            "--description",
            "the payment webhook intermittently drops messages",
            "--json",
        ],
    );
    let id1 = id_from_create(&t1);
    let t2 = run_firetrail(
        root,
        &[
            "task",
            "create",
            "Refactor scheduler",
            "--description",
            "the scheduler module needs a cleanup pass",
            "--json",
        ],
    );
    let id2 = id_from_create(&t2);
    (id1, id2)
}

#[test]
fn search_returns_relevant_hit() {
    let tr = fresh_repo();
    let (id1, _id2) = create_two_records(tr.root());

    let out = run_firetrail(tr.root(), &["search", "payment", "--json"]);
    assert!(out.success(), "search failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["command"], "search");
    let hits = v["data"]["hits"].as_array().expect("hits array");
    assert!(!hits.is_empty(), "expected at least one hit");
    let top = &hits[0];
    assert_eq!(top["id"].as_str().unwrap(), id1, "top hit must be id1");
    assert!(top["score"].as_f64().unwrap() > 0.0);
}

#[test]
fn search_common_word_returns_all_matching_titles() {
    // Regression for firetrail-m1y: searching for a common word ("task")
    // against a corpus of N task records whose titles all contain that
    // word must yield N hits, not 1.
    let tr = fresh_repo();
    let titles = [
        "task alpha",
        "task beta",
        "third task",
        "task in the middle",
        "another task entry",
        "final task wrap",
    ];
    for t in titles {
        let out = run_firetrail(tr.root(), &["task", "create", t, "--json"]);
        assert!(out.success(), "task create failed: {}", out.stderr);
    }
    let out = run_firetrail(tr.root(), &["search", "task", "--json"]);
    assert!(out.success(), "search failed: {}", out.stderr);
    let v = parse_json(&out);
    let hits = v["data"]["hits"].as_array().expect("hits array");
    assert!(
        hits.len() >= titles.len(),
        "expected >= {} hits for `task`; got {} hits: {hits:#?}",
        titles.len(),
        hits.len(),
    );
}

#[test]
fn search_trust_filter_drops_drafts() {
    let tr = fresh_repo();
    // Memory records default to Draft. Filtering by Reviewed should drop
    // them but keep work-tracking kinds (which default to Reviewed in the
    // search engine's trust mapping).
    run_firetrail(
        tr.root(),
        &[
            "task",
            "create",
            "Important payment work",
            "--description",
            "payment payment payment",
            "--json",
        ],
    );
    run_firetrail(
        tr.root(),
        &[
            "memory",
            "create",
            "Payment gotchas",
            "--body",
            "payment quirks notes",
            "--json",
        ],
    );
    let out = run_firetrail(
        tr.root(),
        &["search", "payment", "--trust", "reviewed", "--json"],
    );
    assert!(out.success(), "search failed: {}", out.stderr);
    let v = parse_json(&out);
    let hits = v["data"]["hits"].as_array().expect("hits array");
    assert!(
        hits.iter().all(|h| h["kind"] != "memory"),
        "memory drafts should be filtered out by --trust reviewed: hits={hits:#?}"
    );
}

#[test]
fn similar_returns_other_records() {
    let tr = fresh_repo();
    let (id1, _id2) = create_two_records(tr.root());

    let out = run_firetrail(tr.root(), &["similar", &id1, "--json"]);
    assert!(out.success(), "similar failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["command"], "similar");
    // The other task shares no tokens with id1, so it might not match; but the
    // command must at least return a (possibly empty) hits array and exclude
    // the source.
    let hits = v["data"]["hits"].as_array().expect("hits array");
    for h in hits {
        assert_ne!(h["id"].as_str().unwrap(), id1, "source must be excluded");
    }
}

#[test]
fn prime_for_task_respects_budget() {
    let tr = fresh_repo();
    let (id1, _id2) = create_two_records(tr.root());

    // Generous budget: target record fits.
    let big = run_firetrail(
        tr.root(),
        &["prime", "--task", &id1, "--max-tokens", "8000", "--json"],
    );
    assert!(big.success(), "prime --task failed: {}", big.stderr);
    let v_big = parse_json(&big);
    let used_big = v_big["data"]["total_tokens"]
        .as_u64()
        .expect("total_tokens");
    let budget_big = v_big["data"]["budget"].as_u64().expect("budget");
    assert_eq!(budget_big, 8000);
    assert!(used_big <= budget_big, "must respect budget");
    let items_big = v_big["data"]["items"].as_array().expect("items").len();
    assert!(items_big >= 1, "target must be present");

    // Tiny budget: required items still come through (target), but the
    // omitted list should grow for same-scope candidates.
    let small = run_firetrail(
        tr.root(),
        &["prime", "--task", &id1, "--max-tokens", "20", "--json"],
    );
    assert!(small.success(), "prime small failed: {}", small.stderr);
    let v_small = parse_json(&small);
    let omitted_small = v_small["data"]["omitted"].as_array().expect("omitted");
    let omitted_big = v_big["data"]["omitted"].as_array().expect("omitted");
    assert!(
        omitted_small.len() >= omitted_big.len(),
        "shrinking budget should produce >= as many omitted entries"
    );
}

#[test]
fn prime_for_query_works() {
    let tr = fresh_repo();
    let (_id1, _id2) = create_two_records(tr.root());
    let out = run_firetrail(
        tr.root(),
        &["prime", "--query", "payment webhook", "--json"],
    );
    assert!(out.success(), "prime --query failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["command"], "prime");
    assert!(v["data"]["items"].is_array());
    assert_eq!(v["data"]["query"].as_str().unwrap(), "payment webhook");
}

#[test]
fn index_rebuild_succeeds_and_search_works_after() {
    let tr = fresh_repo();
    let (id1, _id2) = create_two_records(tr.root());

    let rebuild = run_firetrail(tr.root(), &["index", "rebuild", "--json"]);
    assert!(rebuild.success(), "rebuild failed: {}", rebuild.stderr);
    let v = parse_json(&rebuild);
    assert_eq!(v["command"], "index rebuild");
    assert_eq!(v["data"]["action"], "rebuild");
    let search_rows = v["data"]["search_rows_upserted"]
        .as_u64()
        .expect("search_rows_upserted");
    assert!(search_rows >= 2, "expected >=2 records upserted to search");

    let out = run_firetrail(tr.root(), &["search", "payment", "--json"]);
    assert!(out.success(), "search after rebuild failed: {}", out.stderr);
    let v = parse_json(&out);
    let hits = v["data"]["hits"].as_array().expect("hits");
    assert!(!hits.is_empty());
    // The top hit must be the payment-webhook record or one of its audit
    // entries (audit docs for the same record are indexed alongside it and
    // may rank equally, causing either to appear first).
    let top_id = hits[0]["id"].as_str().unwrap();
    assert!(
        top_id == id1 || top_id.starts_with(&format!("audit:{id1}")),
        "expected top hit to be {id1} or its audit doc, got {top_id}"
    );
}

#[test]
fn index_refresh_succeeds() {
    let tr = fresh_repo();
    let (_id1, _id2) = create_two_records(tr.root());
    let out = run_firetrail(tr.root(), &["index", "refresh", "--json"]);
    assert!(out.success(), "refresh failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["action"], "refresh");
}

#[test]
fn daemon_foreground_runs_and_status_reports_running() {
    let tr = fresh_repo();
    let root = tr.root().to_path_buf();

    // Pick a unique socket path under the workspace's sockets dir so we
    // don't collide with any default-path daemon a developer may have.
    let socket = tr
        .firetrail_dir()
        .join("sockets")
        .join(format!("test-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);

    // Spawn the foreground daemon as a child process so we can kill it.
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let mut child = Command::new(bin)
        .args([
            "--workspace",
            root.to_str().unwrap(),
            "daemon",
            "start",
            "--foreground",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("FIRETRAIL_AUTHOR", "tester@firetrail.test")
        .env("FIRETRAIL_CACHE_HOME", env!("CARGO_TARGET_TMPDIR"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait until the socket appears (or time out).
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !socket.exists() {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(socket.exists(), "daemon did not create socket in time");

    // Round-trip the daemon via ft_embed directly using the test socket.
    let status = daemon::status(&socket);
    assert_eq!(status, DaemonStatus::Running, "daemon should be running");

    // Tear down.
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&socket);
}

#[test]
fn create_with_running_daemon_dispatches_embed_under_one_second() {
    // firetrail-0nu: while the embedding daemon is running, a fresh record
    // create must hand off an IndexRecord request and the whole round-trip
    // (storage write + lexical upsert + embed dispatch + ack) must finish
    // within the M3 1-second budget.
    let tr = fresh_repo();
    let root = tr.root().to_path_buf();

    // Spawn the daemon at the *default* socket path so save_record's
    // best-effort dispatch finds it via ws.daemon_socket_path().
    let bin = env!("CARGO_BIN_EXE_firetrail");
    let mut child = Command::new(bin)
        .args([
            "--workspace",
            root.to_str().unwrap(),
            "daemon",
            "start",
            "--foreground",
        ])
        .env("FIRETRAIL_AUTHOR", "tester@firetrail.test")
        .env("FIRETRAIL_CACHE_HOME", env!("CARGO_TARGET_TMPDIR"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // The daemon's real socket path lives under
    // `~/.cache/firetrail/<repo-hash>/` (or `$FIRETRAIL_CACHE_HOME/...`)
    // per firetrail-tij / ADR-0007. Read it back from `daemon status --json`
    // so the test does not have to recompute the repo hash.
    let socket = {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut resolved = None;
        while Instant::now() < deadline {
            let s = run_firetrail(&root, &["daemon", "status", "--json"]);
            if s.success() {
                let v = parse_json(&s);
                if let Some(p) = v["data"]["socket"].as_str() {
                    resolved = Some(std::path::PathBuf::from(p));
                    break;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        resolved.expect("daemon status reported a socket path")
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && daemon::status(&socket) != DaemonStatus::Running {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        daemon::status(&socket),
        DaemonStatus::Running,
        "daemon did not become Running in time"
    );

    let started = Instant::now();
    let out = run_firetrail(
        &root,
        &[
            "task",
            "create",
            "Embed me",
            "--description",
            "Round-trip check for firetrail-0nu",
            "--json",
        ],
    );
    let elapsed = started.elapsed();

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&socket);

    assert!(out.success(), "create failed: {}", out.stderr);
    assert!(
        elapsed < Duration::from_secs(1),
        "create + embed dispatch exceeded 1s: {elapsed:?}"
    );

    let v = parse_json(&out);
    let empty: Vec<serde_json::Value> = Vec::new();
    let warnings = v["data"]["warnings"].as_array().unwrap_or(&empty);
    let dispatch_failures: Vec<&str> = warnings
        .iter()
        .filter_map(|w| w.as_str())
        .filter(|w| w.contains("embed-on-write skipped"))
        .collect();
    assert!(
        dispatch_failures.is_empty(),
        "embed-on-write dispatch should have succeeded; got warnings: {dispatch_failures:?}"
    );
}

#[test]
fn daemon_status_reports_stopped_when_no_socket() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["daemon", "status", "--json"]);
    assert!(out.success(), "status failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["command"], "daemon status");
    assert_eq!(v["data"]["status"], "stopped");
}

#[test]
fn search_hybrid_with_dead_daemon_reports_lexical_mode() {
    // firetrail-urq: when --mode=hybrid is requested but the daemon is
    // unreachable, the result MUST honestly report `mode: lexical` rather
    // than echoing back the requested mode.
    //
    // firetrail-e7z: search now auto-spawns the daemon, so to keep this
    // test exercising the degradation path we configure the workspace to
    // `provider: lexical` — the daemon refuses to start in that mode and
    // the search code falls back to lexical search.
    let tr = fresh_repo();
    let cfg_path = tr.root().join(".firetrail").join("config.yml");
    let cfg = std::fs::read_to_string(&cfg_path).expect("read config.yml");
    std::fs::write(
        &cfg_path,
        cfg.replace("provider: local", "provider: lexical"),
    )
    .expect("rewrite provider to lexical");
    let _ = create_two_records(tr.root());

    let out = run_firetrail(
        tr.root(),
        &[
            "search",
            "payment",
            "--mode",
            "hybrid",
            "--embedder",
            "daemon",
            "--json",
        ],
    );
    assert!(out.success(), "search failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(
        v["data"]["mode"].as_str().unwrap(),
        "lexical",
        "expected mode to degrade to lexical when daemon is down, got {}",
        v["data"]["mode"]
    );
    let warnings = v["data"]["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .filter_map(|w| w.as_str())
            .any(|w| w.contains("daemon embedder unavailable")),
        "expected a daemon-unavailable warning, got {warnings:?}"
    );
}

#[test]
fn doctor_includes_m3_checks() {
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["doctor", "--json"]);
    assert!(out.success(), "doctor failed: {}", out.stderr);
    let v = parse_json(&out);
    let checks = v["data"]["checks"].as_array().expect("checks");
    let ids: Vec<&str> = checks.iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(
        ids.contains(&"embed.cache"),
        "missing embed.cache check: {ids:?}"
    );
    assert!(
        ids.contains(&"embed.daemon"),
        "missing embed.daemon check: {ids:?}"
    );
    assert!(
        ids.contains(&"search.schema"),
        "missing search.schema check: {ids:?}"
    );
}

#[test]
fn daemon_socket_path_is_under_cache_dir() {
    // firetrail-tij: the daemon socket path must resolve under the
    // machine-local cache dir (`$FIRETRAIL_CACHE_HOME/firetrail/<repo-hash>/`
    // or `~/.cache/firetrail/<repo-hash>/`) — not under the workspace's
    // `.firetrail/sockets/` — so deep tmp paths on macOS do not exceed
    // `SUN_LEN`. We don't start the daemon here; we only assert the path
    // shape that `firetrail daemon status --json` reports.
    let tr = fresh_repo();
    let out = run_firetrail(tr.root(), &["daemon", "status", "--json"]);
    assert!(out.success(), "daemon status failed: {}", out.stderr);
    let v = parse_json(&out);
    let socket = v["data"]["socket"]
        .as_str()
        .expect("daemon status reports a socket path");
    assert!(
        socket.contains("/firetrail/") && socket.ends_with("embedd.sock"),
        "socket path should live under `<cache>/firetrail/<repo-hash>/embedd.sock`, got {socket}"
    );
    assert!(
        !socket.contains("/.firetrail/sockets/"),
        "socket path must NOT live under the workspace's `.firetrail/sockets/`, got {socket}"
    );
    // And it must be short enough to avoid macOS `SUN_LEN` (~104 bytes).
    assert!(
        socket.len() < 100,
        "socket path is suspiciously long ({} bytes); macOS SUN_LEN is ~104: {socket}",
        socket.len()
    );
}
