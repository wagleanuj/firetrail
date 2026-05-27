//! [`SearchEngine`] — open / ensure / upsert / search / similar.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use ft_core::{Record, RecordBody, RecordId, RecordKind, TrustState};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::SearchError;
use crate::hit::{HitMode, SearchHit};
use crate::query::{SearchMode, SearchQuery};
use crate::ranking;
use crate::schema;

/// Search backend over a Firetrail index database.
///
/// `SearchEngine` opens the same `SQLite` file as [`ft_index::Index`] (the
/// canonical location is `<workspace>/.firetrail/index.db`) and overlays two
/// virtual tables for full-text and vector search. It owns its own
/// connection — callers that already hold an `Index` keep that handle for
/// the relational queries and create a separate `SearchEngine` for search.
pub struct SearchEngine {
    db_path: PathBuf,
    conn: Connection,
    vec_loaded: bool,
}

impl std::fmt::Debug for SearchEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchEngine")
            .field("db_path", &self.db_path)
            .field("vec_loaded", &self.vec_loaded)
            .finish_non_exhaustive()
    }
}

impl SearchEngine {
    /// Open (or create) the search database at `index_path`.
    ///
    /// `index_path` is the absolute path to the `SQLite` file (callers
    /// typically pass `Index::db_path()`). The file must already exist or be
    /// creatable; `SearchEngine` will not run `ft-index`'s relational
    /// migrations.
    ///
    /// When the `sqlite-vec` feature is on, the constructor attempts to
    /// load the extension. Load failures are logged at `warn` level and the
    /// engine continues in lexical-only mode.
    pub fn open(index_path: &Path) -> Result<Self, SearchError> {
        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(index_path)?;
        // WAL matches ft-index defaults so the two handles cooperate.
        let _: String = conn.query_row("PRAGMA journal_mode = WAL;", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        let vec_loaded = Self::try_load_vec_extension(&conn);

        Ok(Self {
            db_path: index_path.to_path_buf(),
            conn,
            vec_loaded,
        })
    }

    /// Absolute path to the underlying `SQLite` database file.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// True when the `sqlite-vec` extension successfully loaded for this
    /// connection and vector operations will function.
    #[must_use]
    pub fn vector_enabled(&self) -> bool {
        self.vec_loaded
    }

    /// Create FTS5 and (when the extension is loaded) vec0 virtual tables.
    /// Idempotent: safe to call on every process start.
    pub fn ensure_schema(&self) -> Result<(), SearchError> {
        schema::ensure_fts(&self.conn)?;
        if self.vec_loaded {
            schema::ensure_vec(&self.conn)?;
        }
        Ok(())
    }

    /// Upsert the lexical (FTS5) row for `record` and refresh the search
    /// metadata side table (`records_search_meta`) so trust filtering and
    /// trust-weighted ranking see the current `trust` from the record body.
    pub fn upsert_lexical(&self, record: &Record) -> Result<(), SearchError> {
        let (title, body) = record_to_text(record);
        // External-content FTS5 doesn't have a unique key constraint we can
        // upsert against; emulate via DELETE + INSERT.
        let id_str = record.envelope.id.as_str();
        self.conn
            .execute("DELETE FROM records_fts WHERE id = ?1", params![id_str])?;
        self.conn.execute(
            "INSERT INTO records_fts(id, title, body) VALUES (?1, ?2, ?3)",
            params![id_str, title, body],
        )?;
        let trust = trust_for_record(record);
        self.conn.execute(
            "INSERT INTO records_search_meta(id, trust) VALUES (?1, ?2) \
             ON CONFLICT(id) DO UPDATE SET trust = excluded.trust",
            params![id_str, trust_str(trust)],
        )?;
        Ok(())
    }

    /// Upsert the vector for `id`.
    ///
    /// When the `sqlite-vec` feature is **off** or the extension failed to
    /// load, this is a no-op and a `warn`-level message is emitted.
    ///
    /// Errors when `embedding.len() != crate::EMBEDDING_DIM`.
    pub fn upsert_vector(&self, id: &RecordId, embedding: &[f32]) -> Result<(), SearchError> {
        if embedding.len() != crate::EMBEDDING_DIM {
            return Err(SearchError::DimensionMismatch {
                expected: crate::EMBEDDING_DIM,
                actual: embedding.len(),
            });
        }
        if !self.vec_loaded {
            tracing::warn!(
                record = %id,
                "upsert_vector called but sqlite-vec is not loaded; skipping (lexical-only mode)"
            );
            return Ok(());
        }
        self.upsert_vector_inner(id, embedding)
    }

    /// Remove all search index entries (lexical + vector + meta) for `id`.
    pub fn delete(&self, id: &RecordId) -> Result<(), SearchError> {
        let id_str = id.as_str();
        self.conn
            .execute("DELETE FROM records_fts WHERE id = ?1", params![id_str])?;
        self.conn.execute(
            "DELETE FROM records_search_meta WHERE id = ?1",
            params![id_str],
        )?;
        if self.vec_loaded {
            self.conn
                .execute("DELETE FROM records_vec WHERE id_str = ?1", params![id_str])?;
        }
        Ok(())
    }

    /// Run a search query and return ranked hits.
    ///
    /// Hit-mode selection:
    /// - `SearchMode::Auto`  → `Hybrid` when an embedding is supplied **and**
    ///   the vector extension is loaded; else `Lexical`.
    /// - `SearchMode::Lexical` → FTS5 only.
    /// - `SearchMode::Vector`  → vector only; errors when no embedding or no
    ///   extension.
    /// - `SearchMode::Hybrid`  → falls back to whichever signal is available
    ///   when the other is missing (and reports the mode accordingly on each
    ///   hit so callers can render a marker).
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError> {
        let want_vector = matches!(query.mode, SearchMode::Vector)
            || (matches!(query.mode, SearchMode::Hybrid | SearchMode::Auto)
                && query.embedding.is_some()
                && self.vec_loaded);
        let want_lexical = matches!(
            query.mode,
            SearchMode::Lexical | SearchMode::Hybrid | SearchMode::Auto
        );

        if matches!(query.mode, SearchMode::Vector) && !self.vec_loaded {
            return Err(SearchError::VectorUnavailable);
        }
        if matches!(query.mode, SearchMode::Vector) && query.embedding.is_none() {
            return Err(SearchError::VectorUnavailable);
        }

        let now = Utc::now();
        let mut by_id: std::collections::HashMap<String, ScoringRow> =
            std::collections::HashMap::new();

        if want_lexical && !query.text.is_empty() {
            for row in self.fts_query(&query.text, query.limit * 4)? {
                by_id.entry(row.id_str.clone()).or_insert(row);
            }
        }

        if want_vector {
            if let Some(embedding) = &query.embedding {
                for row in self.vec_query(embedding, query.limit * 4)? {
                    by_id
                        .entry(row.id_str.clone())
                        .and_modify(|existing| {
                            existing.vector_sim = row.vector_sim;
                        })
                        .or_insert(row);
                }
            }
        }

        // Decorate with metadata from the `records` table and apply filters.
        let mut hits: Vec<SearchHit> = Vec::new();
        for (id_str, row) in by_id {
            let Some(meta) = self.lookup_meta(&id_str)? else {
                continue;
            };
            if !filters_pass(&meta, query) {
                continue;
            }

            let (mode, score) = combine_score(&row, &meta, now, query.mode, self.vec_loaded);
            let record_id = RecordId::from_string(id_str.clone())
                .map_err(|e| SearchError::Integrity(e.to_string()))?;

            hits.push(SearchHit {
                id: record_id,
                kind: meta.kind,
                title: meta.title,
                score,
                trust: meta.trust,
                mode,
            });
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(query.limit.max(1));
        Ok(hits)
    }

    /// Find records similar to `id`.
    ///
    /// Strategy:
    /// 1. If the vector extension is loaded and `id` has a stored embedding,
    ///    look up the vector and run a vector search (excluding the source).
    /// 2. Otherwise fall back to a lexical search over the source record's
    ///    title + body text.
    ///
    /// Returns up to `limit` hits ranked by descending score. The source
    /// record itself is excluded from the result set.
    pub fn similar(&self, id: &RecordId, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
        let id_str = id.as_str().to_string();
        let limit = limit.max(1);

        // Try vector first.
        if self.vec_loaded {
            if let Some(embedding) = self.fetch_vector(&id_str)? {
                let query = SearchQuery {
                    text: String::new(),
                    mode: SearchMode::Vector,
                    embedding: Some(embedding),
                    limit: limit + 1,
                    ..SearchQuery::default()
                };
                let mut hits = self.search(&query)?;
                hits.retain(|h| h.id.as_str() != id_str);
                hits.truncate(limit);
                return Ok(hits);
            }
        }

        // Lexical fallback: pull the source's title text and run an OR-style
        // FTS query so records sharing *any* of the source tokens surface.
        // (The default `search()` sanitizer is AND-only, which would
        // over-constrain a "similar" query.)
        let text = self.fetch_lexical_text(&id_str)?;
        let match_expr = sanitize_fts_query_or(&text);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }

        let rows = self.fts_query_raw(&match_expr, (limit + 1) * 4)?;
        let now = Utc::now();
        let mut hits: Vec<SearchHit> = Vec::new();
        for row in rows {
            if row.id_str == id_str {
                continue;
            }
            let Some(meta) = self.lookup_meta(&row.id_str)? else {
                continue;
            };
            let score =
                ranking::lexical_only_score(row.lexical_score, meta.trust, meta.updated_at, now);
            let record_id = RecordId::from_string(row.id_str.clone())
                .map_err(|e| SearchError::Integrity(e.to_string()))?;
            hits.push(SearchHit {
                id: record_id,
                kind: meta.kind,
                title: meta.title,
                score,
                trust: meta.trust,
                mode: HitMode::Lexical,
            });
        }
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
    }

    // ── Helpers ────────────────────────────────────────────────────────────

    fn fts_query(&self, text: &str, limit: usize) -> Result<Vec<ScoringRow>, SearchError> {
        self.fts_query_raw(&sanitize_fts_query(text), limit)
    }

    fn fts_query_raw(
        &self,
        match_expr: &str,
        limit: usize,
    ) -> Result<Vec<ScoringRow>, SearchError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, bm25(records_fts) FROM records_fts \
             WHERE records_fts MATCH ?1 \
             ORDER BY bm25(records_fts) LIMIT ?2",
        )?;
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt
            .query_map(params![match_expr, limit_i64], |r| {
                let id: String = r.get(0)?;
                let bm25: f64 = r.get(1)?;
                Ok(ScoringRow {
                    id_str: id,
                    lexical_score: ranking::normalize_bm25(bm25),
                    vector_sim: 0.0,
                    has_lexical: true,
                    has_vector: false,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    #[cfg(feature = "sqlite-vec")]
    fn vec_query(&self, embedding: &[f32], limit: usize) -> Result<Vec<ScoringRow>, SearchError> {
        let bytes = encode_f32_slice(embedding);
        let mut stmt = self.conn.prepare(
            "SELECT id_str, distance FROM records_vec \
             WHERE embedding MATCH ?1 AND k = ?2 \
             ORDER BY distance",
        )?;
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt
            .query_map(params![bytes, limit_i64], |r| {
                let id: String = r.get(0)?;
                let distance: f64 = r.get(1)?;
                // sqlite-vec returns L2 distance by default; convert to a
                // bounded similarity in [0, 1]: 1 / (1 + d).
                let abs = distance.abs();
                let sim = (1.0 / (1.0 + abs)).clamp(0.0, 1.0);
                #[allow(clippy::cast_possible_truncation)]
                let sim_f32 = sim as f32;
                Ok(ScoringRow {
                    id_str: id,
                    lexical_score: 0.0,
                    vector_sim: sim_f32,
                    has_lexical: false,
                    has_vector: true,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    #[cfg(not(feature = "sqlite-vec"))]
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn vec_query(&self, _embedding: &[f32], _limit: usize) -> Result<Vec<ScoringRow>, SearchError> {
        Ok(Vec::new())
    }

    #[cfg(feature = "sqlite-vec")]
    fn upsert_vector_inner(&self, id: &RecordId, embedding: &[f32]) -> Result<(), SearchError> {
        let id_str = id.as_str();
        let bytes = encode_f32_slice(embedding);
        self.conn
            .execute("DELETE FROM records_vec WHERE id_str = ?1", params![id_str])?;
        self.conn.execute(
            "INSERT INTO records_vec(id_str, embedding) VALUES (?1, ?2)",
            params![id_str, bytes],
        )?;
        Ok(())
    }

    #[cfg(not(feature = "sqlite-vec"))]
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn upsert_vector_inner(&self, _id: &RecordId, _embedding: &[f32]) -> Result<(), SearchError> {
        Ok(())
    }

    #[cfg(feature = "sqlite-vec")]
    fn fetch_vector(&self, id_str: &str) -> Result<Option<Vec<f32>>, SearchError> {
        let mut stmt = self
            .conn
            .prepare("SELECT embedding FROM records_vec WHERE id_str = ?1")?;
        let row: Option<Vec<u8>> = stmt
            .query_row(params![id_str], |r| r.get::<_, Vec<u8>>(0))
            .optional()?;
        Ok(row.map(|bytes| decode_f32_bytes(&bytes)))
    }

    #[cfg(not(feature = "sqlite-vec"))]
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn fetch_vector(&self, _id_str: &str) -> Result<Option<Vec<f32>>, SearchError> {
        Ok(None)
    }

    /// Pull the source row's searchable text for `similar()`'s lexical
    /// fallback.
    ///
    /// We use the title only — body text is often long and would force the
    /// AND-tokenized FTS query to over-constrain. Title-only matches the
    /// "find me records about the same topic" intent without requiring a
    /// real semantic model.
    fn fetch_lexical_text(&self, id_str: &str) -> Result<String, SearchError> {
        let mut stmt = self
            .conn
            .prepare("SELECT title FROM records_fts WHERE id = ?1")?;
        let row: Option<String> = stmt
            .query_row(params![id_str], |r| r.get::<_, String>(0))
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    fn lookup_meta(&self, id_str: &str) -> Result<Option<RecordMeta>, SearchError> {
        let mut stmt = self.conn.prepare(
            "SELECT r.kind, r.title, r.updated_at, r.owning_scope, m.trust \
             FROM records r LEFT JOIN records_search_meta m ON m.id = r.id \
             WHERE r.id = ?1",
        )?;
        let row = stmt
            .query_row(params![id_str], |r| {
                let kind_s: String = r.get(0)?;
                let title: String = r.get(1)?;
                let updated_at_s: String = r.get(2)?;
                let owning_scope: Option<String> = r.get(3)?;
                let trust_s: Option<String> = r.get(4)?;
                Ok((kind_s, title, updated_at_s, owning_scope, trust_s))
            })
            .optional()?;

        let Some((kind_s, title, updated_at_s, owning_scope, trust_s)) = row else {
            return Ok(None);
        };
        let kind = parse_kind(&kind_s)
            .ok_or_else(|| SearchError::Integrity(format!("unknown kind `{kind_s}`")))?;
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_s)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| SearchError::Integrity(format!("bad updated_at `{updated_at_s}`: {e}")))?;

        // Prefer the materialised per-record trust (written by
        // `upsert_lexical` from the record body). Records that pre-date the
        // side table — or that have never been upserted — fall back to the
        // kind-driven default so existing scenarios continue to surface.
        let trust = trust_s
            .as_deref()
            .and_then(parse_trust)
            .unwrap_or_else(|| default_trust_for_kind(kind));

        Ok(Some(RecordMeta {
            kind,
            title,
            trust,
            owning_scope,
            updated_at,
        }))
    }

    /// Attempt to load the `sqlite-vec` extension. Returns `true` on success.
    ///
    /// In the current build this always returns `false`: `rusqlite`'s
    /// `load_extension` is marked `unsafe` and `ft-search` runs under the
    /// workspace-wide `unsafe_code = forbid` lint, so we cannot link the
    /// extension from this crate. Wiring the extension is a follow-up
    /// (see crate docs); when added it will live in a sibling crate that
    /// explicitly opts out of the lint and hands us a pre-loaded
    /// [`Connection`] via a future `open_with_connection` constructor.
    #[cfg(feature = "sqlite-vec")]
    fn try_load_vec_extension(_conn: &Connection) -> bool {
        tracing::warn!(
            "sqlite-vec feature is enabled but the extension is not wired up \
             in this build (unsafe FFI gated by workspace lint); operating in \
             lexical-only mode"
        );
        false
    }

    #[cfg(not(feature = "sqlite-vec"))]
    fn try_load_vec_extension(_conn: &Connection) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
struct ScoringRow {
    id_str: String,
    lexical_score: f32,
    vector_sim: f32,
    has_lexical: bool,
    has_vector: bool,
}

#[derive(Debug, Clone)]
struct RecordMeta {
    kind: RecordKind,
    title: String,
    trust: TrustState,
    owning_scope: Option<String>,
    updated_at: DateTime<Utc>,
}

fn combine_score(
    row: &ScoringRow,
    meta: &RecordMeta,
    now: DateTime<Utc>,
    requested_mode: SearchMode,
    vec_loaded: bool,
) -> (HitMode, f32) {
    let both = row.has_lexical && row.has_vector;
    match requested_mode {
        SearchMode::Lexical => (
            HitMode::Lexical,
            ranking::lexical_only_score(row.lexical_score, meta.trust, meta.updated_at, now),
        ),
        SearchMode::Vector => (
            HitMode::Vector,
            ranking::vector_only_score(row.vector_sim, meta.trust, meta.updated_at, now),
        ),
        SearchMode::Hybrid | SearchMode::Auto => {
            if both {
                (
                    HitMode::Hybrid,
                    ranking::hybrid_score(
                        row.vector_sim,
                        row.lexical_score,
                        meta.trust,
                        meta.updated_at,
                        now,
                    ),
                )
            } else if row.has_vector && vec_loaded {
                (
                    HitMode::Vector,
                    ranking::vector_only_score(row.vector_sim, meta.trust, meta.updated_at, now),
                )
            } else {
                (
                    HitMode::Lexical,
                    ranking::lexical_only_score(
                        row.lexical_score,
                        meta.trust,
                        meta.updated_at,
                        now,
                    ),
                )
            }
        }
    }
}

fn filters_pass(meta: &RecordMeta, query: &SearchQuery) -> bool {
    if let Some(min) = query.min_trust {
        if ranking::trust_rank(meta.trust) < ranking::trust_rank(min) {
            return false;
        }
    }
    if !query.kind_filter.is_empty() && !query.kind_filter.contains(&meta.kind) {
        return false;
    }
    if let Some(scope) = &query.scope_filter {
        if meta.owning_scope.as_deref() != Some(scope.as_str()) {
            return false;
        }
    }
    true
}

/// Read the trust state out of a record body. Memory bodies carry a
/// `TrustState` field; work-tracking kinds (epic/task/subtask/bug) have no
/// formal trust and fall back to [`default_trust_for_kind`].
fn trust_for_record(record: &Record) -> TrustState {
    match &record.body {
        RecordBody::Incident(b) => b.trust,
        RecordBody::Finding(b) => b.trust,
        RecordBody::Runbook(b) => b.trust,
        RecordBody::Decision(b) => b.trust,
        RecordBody::Gotcha(b) => b.trust,
        RecordBody::Memory(b) => b.trust,
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            default_trust_for_kind(record.envelope.kind)
        }
    }
}

/// Serialise a `TrustState` into the lowercase tag used in
/// `records_search_meta.trust` (matches the on-the-wire JSON encoding).
fn trust_str(t: TrustState) -> &'static str {
    match t {
        TrustState::Draft => "draft",
        TrustState::Reviewed => "reviewed",
        TrustState::Verified => "verified",
        TrustState::Stale => "stale",
        TrustState::Deprecated => "deprecated",
        TrustState::Superseded => "superseded",
        TrustState::Archived => "archived",
        TrustState::Rejected => "rejected",
        TrustState::Redacted => "redacted",
    }
}

/// Inverse of [`trust_str`]; tolerant of unknown values (returns `None`).
fn parse_trust(s: &str) -> Option<TrustState> {
    Some(match s {
        "draft" => TrustState::Draft,
        "reviewed" => TrustState::Reviewed,
        "verified" => TrustState::Verified,
        "stale" => TrustState::Stale,
        "deprecated" => TrustState::Deprecated,
        "superseded" => TrustState::Superseded,
        "archived" => TrustState::Archived,
        "rejected" => TrustState::Rejected,
        "redacted" => TrustState::Redacted,
        _ => return None,
    })
}

/// Default trust assignment until `ft-trust` surfaces a per-record column.
///
/// Work-tracking kinds carry no formal trust state in M3, so we treat them
/// as `Reviewed` (above `Draft`) — they survive a `min_trust=Reviewed`
/// filter, which mirrors how `firetrail prime` already surfaces them.
/// Memory kinds default to `Draft` so unreviewed notes rank below verified
/// content.
fn default_trust_for_kind(kind: RecordKind) -> TrustState {
    match kind {
        RecordKind::Epic | RecordKind::Task | RecordKind::Subtask | RecordKind::Bug => {
            TrustState::Reviewed
        }
        RecordKind::Incident
        | RecordKind::Finding
        | RecordKind::Runbook
        | RecordKind::Decision
        | RecordKind::Gotcha
        | RecordKind::Memory => TrustState::Draft,
    }
}

fn parse_kind(s: &str) -> Option<RecordKind> {
    Some(match s {
        "epic" => RecordKind::Epic,
        "task" => RecordKind::Task,
        "subtask" => RecordKind::Subtask,
        "bug" => RecordKind::Bug,
        "incident" => RecordKind::Incident,
        "finding" => RecordKind::Finding,
        "runbook" => RecordKind::Runbook,
        "decision" => RecordKind::Decision,
        "gotcha" => RecordKind::Gotcha,
        "memory" => RecordKind::Memory,
        _ => return None,
    })
}

/// Convert a record into `(title, body)` text suitable for FTS5.
fn record_to_text(record: &Record) -> (String, String) {
    let env = &record.envelope;
    let title = env.title.clone();
    let body = match &record.body {
        RecordBody::Epic(e) => e.description.clone(),
        RecordBody::Task(t) => t.description.clone(),
        RecordBody::Subtask(s) => s.description.clone(),
        RecordBody::Bug(b) => {
            let mut parts = Vec::with_capacity(2);
            parts.push(b.description.clone());
            if let Some(svc) = &b.service {
                parts.push(format!("service:{svc}"));
            }
            parts.join("\n")
        }
        RecordBody::Incident(i) => {
            let mut parts = vec![i.summary.clone()];
            if let Some(rc) = &i.root_cause {
                parts.push(rc.clone());
            }
            parts.extend(i.services_affected.iter().cloned());
            parts.join("\n")
        }
        RecordBody::Finding(f) => format!("{}\n{}", f.summary, f.details),
        RecordBody::Runbook(r) => {
            let mut parts = vec![r.title.clone(), r.summary.clone()];
            for step in &r.steps {
                parts.push(step.description.clone());
                if let Some(cmd) = &step.command {
                    parts.push(cmd.clone());
                }
                parts.push(step.expected_outcome.clone());
            }
            parts.join("\n")
        }
        RecordBody::Decision(d) => {
            let mut parts = vec![
                d.title.clone(),
                d.context.clone(),
                d.decision.clone(),
                d.consequences.clone(),
            ];
            parts.extend(d.alternatives_considered.iter().cloned());
            parts.join("\n")
        }
        RecordBody::Gotcha(g) => format!("{}\n{}", g.summary, g.details),
        RecordBody::Memory(m) => {
            let mut parts = vec![m.title.clone(), m.body.clone()];
            parts.extend(m.tags.iter().cloned());
            parts.join("\n")
        }
    };
    (title, body)
}

/// Escape user-supplied text so it parses as an FTS5 phrase query rather
/// than crashing on stray punctuation.
///
/// FTS5 takes a small DSL where unquoted special characters can trip the
/// parser. The safe play is to wrap each token in double quotes and treat
/// the whole thing as an implicit-AND list. Double-quotes inside the token
/// are doubled to escape per FTS5 rules.
fn sanitize_fts_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut first = true;
    for token in text.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        if !first {
            out.push(' ');
        }
        first = false;
        out.push('"');
        for ch in token.chars() {
            if ch == '"' {
                out.push_str("\"\"");
            } else {
                out.push(ch);
            }
        }
        out.push('"');
    }
    if out.is_empty() {
        // Empty MATCH errors in FTS5; substitute a token that matches nothing.
        return "\"\"".to_string();
    }
    out
}

/// FTS5 sanitizer that joins tokens with explicit `OR` (rather than the
/// implicit-AND of [`sanitize_fts_query`]). Used by `similar()` when falling
/// back to lexical: "match records sharing any of these tokens".
///
/// Returns an empty string when the input contains no usable tokens, which
/// callers should treat as "skip the FTS query".
fn sanitize_fts_query_or(text: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        let mut quoted = String::with_capacity(token.len() + 2);
        quoted.push('"');
        for ch in token.chars() {
            if ch == '"' {
                quoted.push_str("\"\"");
            } else {
                quoted.push(ch);
            }
        }
        quoted.push('"');
        tokens.push(quoted);
    }
    tokens.join(" OR ")
}

#[cfg(feature = "sqlite-vec")]
fn encode_f32_slice(slice: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(slice.len() * 4);
    for f in slice {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

#[cfg(feature = "sqlite-vec")]
fn decode_f32_bytes(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
