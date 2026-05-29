//! Integration tests for `ft-search`.
//!
//! These exercise the FTS5 path end-to-end against a real SQLite database in
//! a tempdir. The vector path is feature-gated and not exercised here; those
//! tests live behind `#[cfg(feature = "sqlite-vec")]` once we have an
//! extension binary available in CI.

#![allow(
    missing_docs,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::missing_errors_doc
)]

use std::path::PathBuf;

use chrono::Utc;
use ft_core::Record;
use ft_search::{HitMode, SearchEngine, SearchMode, SearchQuery};
use ft_testkit::{make_bug, make_epic, make_task};
use rusqlite::{Connection, params};
use tempfile::TempDir;

/// Bootstrap a minimal `records` table that mirrors what `ft-index` would
/// produce, so `SearchEngine::lookup_meta` can resolve the metadata it joins
/// during ranking. We only populate the columns the search engine reads.
fn bootstrap_records_table(db_path: &std::path::Path) {
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS records (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            owning_scope TEXT
        );",
    )
    .unwrap();
}

fn insert_record_row(db_path: &std::path::Path, record: &Record) {
    let conn = Connection::open(db_path).unwrap();
    let env = &record.envelope;
    let kind_s = match env.kind {
        ft_core::RecordKind::Epic => "epic",
        ft_core::RecordKind::Task => "task",
        ft_core::RecordKind::Subtask => "subtask",
        ft_core::RecordKind::Bug => "bug",
        ft_core::RecordKind::Incident => "incident",
        ft_core::RecordKind::Finding => "finding",
        ft_core::RecordKind::Runbook => "runbook",
        ft_core::RecordKind::Decision => "decision",
        ft_core::RecordKind::Gotcha => "gotcha",
        ft_core::RecordKind::Memory => "memory",
    };
    conn.execute(
        "INSERT OR REPLACE INTO records(id, kind, title, updated_at, owning_scope) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            env.id.as_str(),
            kind_s,
            env.title,
            env.updated_at.to_rfc3339(),
            env.owning_scope,
        ],
    )
    .unwrap();
}

struct Fixture {
    _dir: TempDir,
    db_path: PathBuf,
    engine: SearchEngine,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");
        bootstrap_records_table(&db_path);
        let engine = SearchEngine::open(&db_path).unwrap();
        engine.ensure_schema().unwrap();
        Self {
            _dir: dir,
            db_path,
            engine,
        }
    }

    fn ingest(&self, record: &Record) {
        insert_record_row(&self.db_path, record);
        self.engine.upsert_lexical(record).unwrap();
    }
}

#[test]
fn ensure_schema_creates_fts_table() {
    let fix = Fixture::new();
    // sqlite_master should now show the FTS table.
    let conn = Connection::open(&fix.db_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'records_fts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "records_fts virtual table should exist");
}

#[test]
fn ensure_schema_is_idempotent() {
    let fix = Fixture::new();
    // Calling ensure_schema again must not error.
    fix.engine.ensure_schema().unwrap();
    fix.engine.ensure_schema().unwrap();
}

#[test]
fn upsert_then_search_finds_body_term() {
    let fix = Fixture::new();
    let task = make_task()
        .title("Tune the cache eviction policy")
        .description("Replace the existing LRU implementation with a clock-pro algorithm.")
        .build();
    fix.ingest(&task);

    let other = make_task()
        .title("Ship onboarding flow")
        .description("Hook up the welcome screen and analytics.")
        .build();
    fix.ingest(&other);

    let hits = fix.engine.search(&SearchQuery::new("clock-pro")).unwrap();
    assert!(!hits.is_empty(), "should match record by body term");
    assert_eq!(hits[0].id.as_record_id(), Some(&task.envelope.id));
    assert_eq!(hits[0].mode, HitMode::Lexical);
}

#[test]
fn search_returns_mode_marker_for_lexical() {
    let fix = Fixture::new();
    let bug = make_bug()
        .title("Memory leak in image cache")
        .description("Vips backend retains decoded buffers across requests.")
        .build();
    fix.ingest(&bug);

    let q = SearchQuery {
        text: "vips".into(),
        mode: SearchMode::Lexical,
        ..SearchQuery::default()
    };
    let hits = fix.engine.search(&q).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].mode, HitMode::Lexical);
}

#[test]
fn min_trust_filter_excludes_drafts() {
    let fix = Fixture::new();
    // Tasks default to `Reviewed` in our trust mapping. We need a memory-kind
    // body to exercise the Draft path — but memory kinds aren't writable via
    // the M1 testkit builders. We simulate by inserting a runbook row
    // manually + indexing it through upsert_lexical with a constructed record.
    // For this test we lean on the kind→trust mapping: tasks survive
    // min_trust=Reviewed, but if we min_trust=Verified we should drop them.
    let task = make_task()
        .title("Investigate flaky test")
        .description("payment-service intermittently fails the smoke suite.")
        .build();
    fix.ingest(&task);

    // No filter → match.
    let hits = fix
        .engine
        .search(&SearchQuery::new("payment-service"))
        .unwrap();
    assert_eq!(hits.len(), 1);

    // min_trust = Verified → tasks (mapped to Reviewed) drop.
    let q = SearchQuery {
        text: "payment-service".into(),
        min_trust: Some(ft_core::TrustState::Verified),
        ..SearchQuery::default()
    };
    let hits = fix.engine.search(&q).unwrap();
    assert!(
        hits.is_empty(),
        "Verified filter should drop Reviewed tasks"
    );
}

#[test]
fn kind_filter_restricts_results() {
    let fix = Fixture::new();
    let epic = make_epic()
        .title("Reliability refactor")
        .description("Sweep flaky regions across the platform.")
        .build();
    let bug = make_bug()
        .title("Platform crash on startup")
        .description("Workers crash sweeping configuration files on boot.")
        .build();
    fix.ingest(&epic);
    fix.ingest(&bug);

    // Both records contain "reliability" / "platform"; filter to Bug only.
    let q = SearchQuery {
        text: "platform".into(),
        kind_filter: vec![ft_search::IndexKind::Record(ft_core::RecordKind::Bug)],
        ..SearchQuery::default()
    };
    let hits = fix.engine.search(&q).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id.as_record_id(), Some(&bug.envelope.id));
}

#[test]
fn delete_removes_from_search() {
    let fix = Fixture::new();
    let task = make_task()
        .title("Add unique widget")
        .description("Sprinkle whimsicality across the homepage.")
        .build();
    fix.ingest(&task);

    let hits = fix
        .engine
        .search(&SearchQuery::new("whimsicality"))
        .unwrap();
    assert_eq!(hits.len(), 1);

    fix.engine.delete(&ft_search::DocId::Record(task.envelope.id.clone())).unwrap();
    let hits = fix
        .engine
        .search(&SearchQuery::new("whimsicality"))
        .unwrap();
    assert!(hits.is_empty(), "delete should remove FTS row");
}

#[test]
fn similar_lexical_returns_other_hits_not_self() {
    let fix = Fixture::new();
    let a = make_task()
        .title("Distributed cache warmup")
        .description("Pre-populate redis keys on deploy to avoid cold-start latency.")
        .build();
    let b = make_task()
        .title("Cache warmup runbook")
        .description("Document the cold-start mitigation: pre-populate redis on deploy.")
        .build();
    let unrelated = make_task()
        .title("Onboarding survey copy")
        .description("Rewrite the welcome questionnaire.")
        .build();
    fix.ingest(&a);
    fix.ingest(&b);
    fix.ingest(&unrelated);

    let hits = fix.engine.similar(&a.envelope.id, 5).unwrap();
    let ids: Vec<String> = hits.iter().map(|h| h.id.as_storage_str()).collect();
    assert!(
        !ids.contains(&a.envelope.id.as_str().to_string()),
        "similar() must exclude the source record from lexical fallback"
    );
    // We expect `b` (cache warmup) to show up; the unrelated row may or may
    // not depending on FTS scoring noise, but `b` should be present.
    assert!(
        ids.contains(&b.envelope.id.as_str().to_string()),
        "expected the related cache-warmup task to show up, got {ids:?}"
    );
}

#[test]
fn vector_only_errors_when_extension_missing() {
    let fix = Fixture::new();
    if fix.engine.vector_enabled() {
        // CI has the extension wired up; the negative path can't be exercised
        // — skip rather than fail.
        return;
    }
    let q = SearchQuery {
        text: "anything".into(),
        mode: SearchMode::Vector,
        embedding: Some(vec![0.0; ft_search::EMBEDDING_DIM]),
        ..SearchQuery::default()
    };
    let err = fix
        .engine
        .search(&q)
        .expect_err("Vector mode must error without extension");
    matches!(err, ft_search::SearchError::VectorUnavailable);
}

#[test]
fn upsert_vector_is_noop_without_extension() {
    let fix = Fixture::new();
    if fix.engine.vector_enabled() {
        return;
    }
    let task = make_task().title("vec noop").description("x").build();
    fix.ingest(&task);
    // Should be a no-op + warn, not an error.
    fix.engine
        .upsert_vector(&ft_search::DocId::Record(task.envelope.id.clone()), &vec![0.0; ft_search::EMBEDDING_DIM])
        .unwrap();
}

#[test]
fn dimension_mismatch_rejects() {
    let fix = Fixture::new();
    let task = make_task().title("dim").description("x").build();
    fix.ingest(&task);
    let err = fix
        .engine
        .upsert_vector(&ft_search::DocId::Record(task.envelope.id.clone()), &[0.0, 1.0, 2.0])
        .expect_err("wrong dimension must error even in lexical-only build");
    assert!(matches!(
        err,
        ft_search::SearchError::DimensionMismatch { .. }
    ));
}

#[test]
fn search_orders_by_descending_score() {
    let fix = Fixture::new();
    // Two records, one with the term in title, one with the term buried in
    // body. FTS5 bm25 should rank the title-hit higher; after normalization
    // and trust multiplication the order should hold.
    let strong = make_task()
        .title("Kubernetes upgrade plan")
        .description("Rolling upgrade of the worker nodes.")
        .build();
    let weak = make_task()
        .title("Quarterly review")
        .description("Mention the kubernetes upgrade in the recap.")
        .build();
    fix.ingest(&strong);
    fix.ingest(&weak);

    let hits = fix.engine.search(&SearchQuery::new("kubernetes")).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(
        hits[0].score >= hits[1].score,
        "results should be ordered descending: got {hits:?}"
    );
}

/// Build a 384-d unit vector pointing along `axis` (a one-hot embedding).
/// Distinct axes give orthogonal vectors, so nearest-neighbour ordering is
/// unambiguous in these tests.
#[cfg(feature = "sqlite-vec")]
fn one_hot(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0_f32; ft_search::EMBEDDING_DIM];
    v[axis] = 1.0;
    v
}

#[cfg(feature = "sqlite-vec")]
#[test]
fn vector_enabled_when_feature_on() {
    let fix = Fixture::new();
    assert!(
        fix.engine.vector_enabled(),
        "with the sqlite-vec feature compiled in, the extension must load and \
         vector_enabled() must be true"
    );
    // The vec0 virtual table should exist after ensure_schema().
    let conn = Connection::open(&fix.db_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'records_vec'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "records_vec virtual table should exist");
}

#[cfg(feature = "sqlite-vec")]
#[test]
fn vector_search_ranks_nearest_first() {
    let fix = Fixture::new();

    let near = make_task()
        .title("Alpha record")
        .description("text body for alpha")
        .build();
    let far = make_task()
        .title("Beta record")
        .description("text body for beta")
        .build();
    fix.ingest(&near);
    fix.ingest(&far);

    // Orthogonal embeddings: `near` on axis 0, `far` on axis 1.
    fix.engine
        .upsert_vector(&ft_search::DocId::Record(near.envelope.id.clone()), &one_hot(0))
        .unwrap();
    fix.engine
        .upsert_vector(&ft_search::DocId::Record(far.envelope.id.clone()), &one_hot(1))
        .unwrap();

    // Query embedding sits on axis 0 → `near` must be the closest neighbour.
    let q = SearchQuery {
        text: String::new(),
        mode: SearchMode::Vector,
        embedding: Some(one_hot(0)),
        ..SearchQuery::default()
    };
    let hits = fix.engine.search(&q).unwrap();
    assert!(!hits.is_empty(), "vector search should return hits");
    assert_eq!(
        hits[0].id.as_record_id(),
        Some(&near.envelope.id),
        "the record whose embedding matches the query must rank first"
    );
    assert_eq!(hits[0].mode, HitMode::Vector);
}

#[test]
fn meta_table_has_synthetic_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("index.db");
    let engine = ft_search::SearchEngine::open(&db).unwrap();
    engine.ensure_schema().unwrap();
    // Re-open to prove the migration is idempotent across connections
    // (drop the first handle so this is a clean sequential re-open).
    drop(engine);
    let engine2 = ft_search::SearchEngine::open(&db).unwrap();
    engine2.ensure_schema().unwrap();

    let cols = engine2.debug_meta_columns().unwrap();
    for expected in ["id", "trust", "kind", "title", "updated_at", "owning_scope"] {
        assert!(cols.contains(&expected.to_string()), "missing column {expected}");
    }
}

#[test]
fn now_used_for_recency_is_reasonable() {
    // Smoke: just make sure searching against an empty index returns Ok and
    // doesn't panic on the recency math when there are zero hits.
    let fix = Fixture::new();
    let hits = fix
        .engine
        .search(&SearchQuery::new("nothing matches"))
        .unwrap();
    assert!(hits.is_empty());
    let _ = Utc::now(); // silence unused-import lint when refactored.
}

#[test]
fn synthetic_doc_resolves_without_records_row() {
    let fix = Fixture::new();
    // A scope-kind synthetic doc — note: NO insert_record_row call.
    let doc = ft_search::IndexDoc {
        id: ft_search::DocId::Synthetic {
            kind: ft_search::IndexKind::Scope,
            key: "apps/checkout".to_string(),
        },
        kind: ft_search::IndexKind::Scope,
        title: "Checkout".to_string(),
        body: "apps/checkout payments owner".to_string(),
        trust: ft_core::TrustState::Verified,
        owning_scope: Some("apps/checkout".to_string()),
        updated_at: chrono::Utc::now(),
    };
    fix.engine.upsert_document(&doc).unwrap();

    let hits = fix.engine.search(&ft_search::SearchQuery::new("payments")).unwrap();
    assert_eq!(hits.len(), 1, "synthetic scope doc should be searchable with no records row");
    assert_eq!(hits[0].kind, ft_search::IndexKind::Scope);
    assert_eq!(hits[0].trust, ft_core::TrustState::Verified);
    assert_eq!(hits[0].id.as_storage_str(), "scope:apps/checkout");
}
