//! Integration tests for `firetrail migrate embeddings` (firetrail-vpn).

mod common;

use common::{fresh_repo, parse_json, run_firetrail};

#[test]
fn migrate_embeddings_writes_jsonl_artifact_and_is_resumable() {
    let tr = fresh_repo();
    // Pin the mock provider: the default is now `provider: local`, which
    // migrate refuses without a real model_dir (it won't bake mock vectors
    // into a migration artifact). This test exercises the embed + write loop
    // + resumability, for which the deterministic mock embedder is enough.
    let cfg_path = tr.root().join(".firetrail").join("config.yml");
    let cfg = std::fs::read_to_string(&cfg_path).expect("read config.yml");
    std::fs::write(&cfg_path, cfg.replace("provider: local", "provider: mock"))
        .expect("pin mock provider");

    // Seed a small corpus.
    for title in ["alpha task", "beta task", "gamma task"] {
        let out = run_firetrail(tr.root(), &["task", "create", title, "--json"]);
        assert!(out.success(), "task create failed: {}", out.stderr);
    }

    let artifact = tr.root().join("embeddings.jsonl");

    // First run with the mock provider — exercises the full embed + write
    // loop without needing ONNX.
    let out = run_firetrail(
        tr.root(),
        &[
            "migrate",
            "embeddings",
            "--to",
            "bge-small-en-v1.5",
            "--dim",
            "384",
            "--output",
            artifact.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(out.success(), "migrate failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["command"], "migrate embeddings");
    assert_eq!(v["data"]["written"], 3);
    assert_eq!(v["data"]["skipped"], 0);
    assert_eq!(v["data"]["total"], 3);
    assert_eq!(v["data"]["model_id"], "bge-small-en-v1.5");
    assert_eq!(v["data"]["dim"], 384);

    let first_sha = v["data"]["artifact_sha256"]
        .as_str()
        .expect("artifact_sha256")
        .to_string();
    assert_eq!(first_sha.len(), 64, "sha256 hex should be 64 chars");

    // Lines actually present in the artifact.
    let body = std::fs::read_to_string(&artifact).expect("read artifact");
    let line_count = body.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(line_count, 3, "artifact should have one line per record");

    // Resume: with the artifact already populated, a second run must write
    // zero and skip all three. The final artifact_sha256 must match — that
    // is the determinism guarantee.
    let out = run_firetrail(
        tr.root(),
        &[
            "migrate",
            "embeddings",
            "--to",
            "bge-small-en-v1.5",
            "--dim",
            "384",
            "--output",
            artifact.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(out.success(), "resume migrate failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(
        v["data"]["written"], 0,
        "expected zero new writes on resume"
    );
    assert_eq!(v["data"]["skipped"], 3);
    assert_eq!(
        v["data"]["artifact_sha256"].as_str().unwrap(),
        first_sha,
        "artifact_sha256 must be identical across runs (determinism)"
    );

    // --force blows the file away and re-emits a fresh artifact with the
    // same content / hash.
    let out = run_firetrail(
        tr.root(),
        &[
            "migrate",
            "embeddings",
            "--to",
            "bge-small-en-v1.5",
            "--dim",
            "384",
            "--output",
            artifact.to_str().unwrap(),
            "--force",
            "--json",
        ],
    );
    assert!(out.success(), "force migrate failed: {}", out.stderr);
    let v = parse_json(&out);
    assert_eq!(v["data"]["written"], 3);
    assert_eq!(v["data"]["skipped"], 0);
    assert_eq!(
        v["data"]["artifact_sha256"].as_str().unwrap(),
        first_sha,
        "deterministic hash must survive --force"
    );
}

#[test]
fn migrate_embeddings_refuses_lexical_provider() {
    let tr = fresh_repo();
    let cfg_path = tr.root().join(".firetrail").join("config.yml");
    let cfg = std::fs::read_to_string(&cfg_path).expect("read config.yml");
    std::fs::write(
        &cfg_path,
        cfg.replace("provider: local", "provider: lexical"),
    )
    .expect("write lexical config");

    let artifact = tr.root().join("out.jsonl");
    let out = run_firetrail(
        tr.root(),
        &[
            "migrate",
            "embeddings",
            "--to",
            "bge-small-en-v1.5",
            "--output",
            artifact.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(
        !out.success(),
        "expected migrate to fail under lexical provider"
    );
    assert!(
        out.stderr.contains("lexical") || out.stdout.contains("lexical"),
        "expected error mentioning lexical, got stderr={:?} stdout={:?}",
        out.stderr,
        out.stdout
    );
}
