//! [`SearchEngine`] — open / ensure / upsert / search / similar.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use ft_core::{Record, RecordBody, RecordId, RecordKind, TrustState};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::SearchError;
use crate::hit::{HitMode, SearchHit};
use crate::kind::{DocId, IndexKind};
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
        // Register the sqlite-vec auto-extension *before* opening the
        // connection: `sqlite3_auto_extension` only attaches the `vec0` module
        // to connections created afterward. Process-global and idempotent.
        #[cfg(feature = "sqlite-vec")]
        let _ = ft_vec::register();
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

    /// Upsert a document's lexical row and full search metadata. The metadata
    /// is written self-sufficiently (`kind`/`title`/`updated_at`/`owning_scope`/`trust`)
    /// so synthetic docs resolve without a `records` row.
    pub fn upsert_document(&self, doc: &IndexDoc) -> Result<(), SearchError> {
        let id_str = doc.id.as_storage_str();
        self.conn
            .execute("DELETE FROM records_fts WHERE id = ?1", params![id_str])?;
        self.conn.execute(
            "INSERT INTO records_fts(id, title, body) VALUES (?1, ?2, ?3)",
            params![id_str, doc.title, doc.body],
        )?;
        self.conn.execute(
            "INSERT INTO records_search_meta(id, trust, kind, title, updated_at, owning_scope) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(id) DO UPDATE SET \
               trust = excluded.trust, kind = excluded.kind, title = excluded.title, \
               updated_at = excluded.updated_at, owning_scope = excluded.owning_scope",
            params![
                id_str,
                trust_str(doc.trust),
                doc.kind.label(),
                doc.title,
                doc.updated_at.to_rfc3339(),
                doc.owning_scope,
            ],
        )?;
        Ok(())
    }

    /// Upsert the lexical row + metadata for a record. Thin wrapper over
    /// [`Self::upsert_document`].
    pub fn upsert_lexical(&self, record: &Record) -> Result<(), SearchError> {
        self.upsert_lexical_inner(record, record_to_text(record))
    }

    /// Like [`Self::upsert_lexical`], but for file-backed `Doc` records indexes
    /// the linked `.md` file's contents (resolved against `root`) instead of the
    /// stored summary. A missing file degrades to the summary fallback.
    pub fn upsert_lexical_with_root(
        &self,
        record: &Record,
        root: &std::path::Path,
    ) -> Result<(), SearchError> {
        self.upsert_lexical_inner(record, record_to_text_with_root(record, Some(root)))
    }

    fn upsert_lexical_inner(
        &self,
        record: &Record,
        (title, body): (String, String),
    ) -> Result<(), SearchError> {
        let doc = IndexDoc {
            id: DocId::Record(record.envelope.id.clone()),
            kind: IndexKind::Record(record.envelope.kind),
            title,
            body,
            trust: trust_for_record(record),
            owning_scope: record.envelope.owning_scope.clone(),
            updated_at: record.envelope.updated_at,
        };
        self.upsert_document(&doc)
    }

    /// Upsert the vector for `id`.
    ///
    /// When the `sqlite-vec` feature is **off** or the extension failed to
    /// load, this is a no-op and a `warn`-level message is emitted.
    ///
    /// Errors when `embedding.len() != crate::EMBEDDING_DIM`.
    pub fn upsert_vector(&self, id: &DocId, embedding: &[f32]) -> Result<(), SearchError> {
        if embedding.len() != crate::EMBEDDING_DIM {
            return Err(SearchError::DimensionMismatch {
                expected: crate::EMBEDDING_DIM,
                actual: embedding.len(),
            });
        }
        if !self.vec_loaded {
            tracing::warn!(
                record = %id.as_storage_str(),
                "upsert_vector called but sqlite-vec is not loaded; skipping (lexical-only mode)"
            );
            return Ok(());
        }
        self.upsert_vector_inner(id, embedding)
    }

    /// Remove all search index entries (lexical + vector + meta) for `id`.
    pub fn delete(&self, id: &DocId) -> Result<(), SearchError> {
        let id_str = id.as_storage_str();
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
            let doc_id = DocId::parse(&id_str);

            hits.push(SearchHit {
                id: doc_id,
                kind: meta.kind,
                title: meta.title,
                score,
                trust: meta.trust,
                owning_scope: meta.owning_scope,
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
                hits.retain(|h| h.id.as_storage_str() != id_str);
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
                ranking::lexical_only_score(row.lexical_score, meta.trust, meta.updated_at, now)
                    * ranking::kind_weight(meta.kind);
            hits.push(SearchHit {
                id: DocId::parse(&row.id_str),
                kind: meta.kind,
                title: meta.title,
                score,
                trust: meta.trust,
                owning_scope: meta.owning_scope,
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
    fn upsert_vector_inner(&self, id: &DocId, embedding: &[f32]) -> Result<(), SearchError> {
        let id_str = id.as_storage_str();
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
    fn upsert_vector_inner(&self, _id: &DocId, _embedding: &[f32]) -> Result<(), SearchError> {
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
            "SELECT \
               COALESCE(r.kind, m.kind), \
               COALESCE(r.title, m.title), \
               COALESCE(r.updated_at, m.updated_at), \
               COALESCE(r.owning_scope, m.owning_scope), \
               m.trust \
             FROM records_search_meta m \
             LEFT JOIN records r ON r.id = m.id \
             WHERE m.id = ?1",
        )?;
        let row = stmt
            .query_row(params![id_str], |r| {
                let kind_s: Option<String> = r.get(0)?;
                let title: Option<String> = r.get(1)?;
                let updated_at_s: Option<String> = r.get(2)?;
                let owning_scope: Option<String> = r.get(3)?;
                let trust_s: Option<String> = r.get(4)?;
                Ok((kind_s, title, updated_at_s, owning_scope, trust_s))
            })
            .optional()?;

        let Some((kind_s, title, updated_at_s, owning_scope, trust_s)) = row else {
            return Ok(None);
        };
        let Some(kind_s) = kind_s else {
            return Ok(None);
        };
        let Some(kind) = IndexKind::parse_label(&kind_s) else {
            return Ok(None);
        }; // unknown kind label → skip, don't error
        let Some(updated_at_s) = updated_at_s else {
            return Ok(None);
        };
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_s)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| SearchError::Integrity(format!("bad updated_at `{updated_at_s}`: {e}")))?;

        let trust = trust_s
            .as_deref()
            .and_then(parse_trust)
            .unwrap_or_else(|| default_trust_for_index_kind(kind));

        Ok(Some(RecordMeta {
            kind,
            title: title.unwrap_or_default(),
            trust,
            owning_scope,
            updated_at,
        }))
    }

    /// List the column names of `records_search_meta`.
    ///
    /// Used by integration tests to verify that schema migrations have run
    /// correctly. Not intended for production use.
    #[doc(hidden)]
    pub fn debug_meta_columns(&self) -> Result<Vec<String>, SearchError> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA table_info(records_search_meta)")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Verify the `sqlite-vec` extension is available on `conn`. Returns
    /// `true` when vector operations will function.
    ///
    /// Registration of the (statically linked) extension happens in
    /// [`ft_vec::register`], called from [`SearchEngine::open`] before the
    /// connection is opened — the single `unsafe` `sqlite3_auto_extension`
    /// call is isolated there so this crate stays under the workspace-wide
    /// `unsafe_code = forbid` lint. Here we simply confirm the `vec0` module
    /// resolved on this connection by probing `vec_version()`; on any failure
    /// we log and fall back to lexical-only mode.
    #[cfg(feature = "sqlite-vec")]
    fn try_load_vec_extension(conn: &Connection) -> bool {
        match conn.query_row("SELECT vec_version()", [], |r| r.get::<_, String>(0)) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "sqlite-vec feature is enabled but the extension did not \
                     register; operating in lexical-only mode"
                );
                false
            }
        }
    }

    #[cfg(not(feature = "sqlite-vec"))]
    fn try_load_vec_extension(_conn: &Connection) -> bool {
        false
    }
}

/// A unit of searchable text + metadata, independent of `ft_core::Record`.
/// Records and synthetic domains (scope/identity/audit) both lower to this.
#[derive(Debug, Clone)]
pub struct IndexDoc {
    /// Document id.
    pub id: DocId,
    /// Document kind.
    pub kind: IndexKind,
    /// Short title (FTS `title` column + surfaced on the hit).
    pub title: String,
    /// Body text (FTS `body` column).
    pub body: String,
    /// Trust state written to `records_search_meta.trust`.
    pub trust: TrustState,
    /// Owning scope (filterable), if any.
    pub owning_scope: Option<String>,
    /// Last-updated timestamp used by recency ranking.
    pub updated_at: DateTime<Utc>,
}

impl IndexDoc {
    /// Text handed to the embedder (title + body). Mirrors the FTS content so
    /// lexical and vector indexes see the same source.
    #[must_use]
    pub fn embed_text(&self) -> String {
        if self.body.is_empty() {
            self.title.clone()
        } else if self.title.is_empty() {
            self.body.clone()
        } else {
            format!("{}\n\n{}", self.title, self.body)
        }
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
    kind: IndexKind,
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
    let (mode, score) = base_combine_score(row, meta, now, requested_mode, vec_loaded);
    // firetrail-8z0m.7: down-rank audit echoes so they sort below the parent
    // record / other domains while staying searchable.
    (mode, score * ranking::kind_weight(meta.kind))
}

fn base_combine_score(
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
        RecordBody::Doc(b) => b.trust,
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
        | RecordKind::Memory
        | RecordKind::Doc => TrustState::Draft,
    }
}

/// Trust fallback for an indexed doc with no materialised `trust` column.
/// Records delegate to the existing `default_trust_for_kind`; scopes/identities
/// are authoritative configuration → `Verified`; audit is a fallback
/// (`Reviewed`) since audit docs always carry an inherited trust at write time.
fn default_trust_for_index_kind(kind: IndexKind) -> TrustState {
    match kind {
        IndexKind::Record(k) => default_trust_for_kind(k),
        IndexKind::Scope | IndexKind::Identity => TrustState::Verified,
        IndexKind::Audit => TrustState::Reviewed,
    }
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
        RecordBody::Doc(d) => doc_index_body(d, None),
    };
    (title, body)
}

/// Like [`record_to_text`] but resolves a file-backed `Doc`'s body from the
/// linked `.md` file (against `root`) when available.
fn record_to_text_with_root(record: &Record, root: Option<&std::path::Path>) -> (String, String) {
    if let RecordBody::Doc(d) = &record.body {
        return (record.envelope.title.clone(), doc_index_body(d, root));
    }
    record_to_text(record)
}

/// FTS body for a `Doc`: the linked file's contents when readable, else the
/// stored title + summary so a broken link stays lexically findable.
fn doc_index_body(d: &ft_core::Doc, root: Option<&std::path::Path>) -> String {
    if let Some(root) = root {
        if let Ok(content) = std::fs::read_to_string(root.join(&d.path)) {
            return content;
        }
    }
    format!("{}\n{}", d.title, d.summary)
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
