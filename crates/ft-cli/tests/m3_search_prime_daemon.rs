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
    assert_eq!(hits[0]["id"].as_str().unwrap(), id1);
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
    std::fs::write(&cfg_path, cfg.replace("provider: mock", "provider: lexical"))
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
