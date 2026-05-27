//! SQLite-backed content-hash embedding cache.
//!
//! Rows are keyed by `(model_id, model_version, content_hash)` so switching
//! worktrees, or re-importing unchanged source files, doesn't re-embed
//! (ADR-0007). The cache database lives at the **machine-local** path
//! `$HOME/.cache/firetrail/<repo-hash>/embeddings.db` (or
//! `$FIRETRAIL_CACHE_HOME/<repo-hash>/embeddings.db` when overridden) — see
//! [`repo_cache_dir`] — so multiple worktrees of the same repo share one
//! cache. `<repo-hash>` is BLAKE3 of the repo's origin remote URL when
//! available, else the canonical absolute path of the repo root.
//!
//! Each row carries an `integrity_checksum` — a BLAKE3 hex digest of
//! `(model_id || model_version || content_hash || embedding_bytes)` with
//! `0x00` separators. [`EmbeddingCache::lookup`] validates the checksum on
//! read; [`EmbeddingCache::verify_integrity`] sweeps the entire table.
//!
//! ## Schema migration
//!
//! Tracked via `PRAGMA user_version`. Bumping the version drops the prior
//! `embeddings` table — pre-M3 the cache only contains scaffolding
//! (`MockEmbedder`) vectors, so re-embedding is cheap. See ADR-0007
//! "Model upgrades" for the full migration story when real models land.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

/// Current on-disk schema version. Bumped when the row layout or checksum
/// definition changes; bumping triggers a drop+recreate inside
/// [`EmbeddingCache::open`].
const SCHEMA_VERSION: i32 = 2;

/// Errors returned by [`EmbeddingCache`].
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// Filesystem failure under the cache directory.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// SQLite-level failure.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// A row's integrity checksum did not match its recomputed value.
    #[error("integrity check failed for ({model_id}, {model_version}, {content_hash})")]
    IntegrityFailed {
        /// Model id of the corrupt row.
        model_id: String,
        /// Model version of the corrupt row.
        model_version: String,
        /// Content hash of the corrupt row.
        content_hash: String,
    },
    /// `$HOME` was not set, so we cannot resolve the machine-local cache
    /// root. Set `$FIRETRAIL_CACHE_HOME` to override.
    #[error("cannot resolve machine-local cache directory: {0}")]
    NoHome(String),
}

/// Summary returned by [`EmbeddingCache::verify_integrity`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    /// Total rows scanned.
    pub scanned: usize,
    /// Rows whose checksum mismatched.
    pub bad: Vec<IntegrityIssue>,
}

/// A single integrity-check failure entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityIssue {
    /// Model id of the offending row.
    pub model_id: String,
    /// Model version of the offending row.
    pub model_version: String,
    /// Content hash of the offending row.
    pub content_hash: String,
    /// Checksum recorded in the row.
    pub recorded: String,
    /// Checksum recomputed from row contents.
    pub recomputed: String,
}

/// One row returned by [`EmbeddingCache::sample_for_reembed`]. Carries the
/// cached vector along with its partition keys so the caller can re-embed
/// the corresponding text (from source storage) and compare.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleRow {
    /// Model id of the row (cache partition key).
    pub model_id: String,
    /// Model version of the row (cache partition key).
    pub model_version: String,
    /// Content hash of the row (cache partition key).
    pub content_hash: String,
    /// Cached embedding vector.
    pub embedding: Vec<f32>,
}

/// Content-hash keyed embedding cache.
#[derive(Debug)]
pub struct EmbeddingCache {
    db_path: PathBuf,
    conn: Connection,
}

impl EmbeddingCache {
    /// Open (or create) the cache at the given absolute `SQLite` file path.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, CacheError> {
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        apply_pragmas(&conn)?;
        init_schema(&conn)?;
        Ok(Self { db_path, conn })
    }

    /// Open the **machine-local** cache for the repo rooted at
    /// `workspace_root`.
    ///
    /// Resolves to `<cache_home>/firetrail/<repo-hash>/embeddings.db` where
    /// `<cache_home>` is `$FIRETRAIL_CACHE_HOME` when set, otherwise
    /// `$HOME/.cache`, and `<repo-hash>` is derived via [`repo_identity`].
    ///
    /// Multiple worktrees of the same repo (same origin URL) share this
    /// cache — switching worktrees does not re-embed (ADR-0007).
    pub fn open_under(workspace_root: impl AsRef<Path>) -> Result<Self, CacheError> {
        let dir = repo_cache_dir(workspace_root.as_ref())?;
        Self::open(dir.join("embeddings.db"))
    }

    /// Absolute path to the on-disk database.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Look up a cached embedding. Validates the row's integrity checksum.
    ///
    /// Returns `Ok(None)` if the row is absent; returns
    /// [`CacheError::IntegrityFailed`] if a row exists but its checksum
    /// doesn't match.
    pub fn lookup(
        &self,
        model_id: &str,
        model_version: &str,
        content_hash: &str,
    ) -> Result<Option<Vec<f32>>, CacheError> {
        let row: Option<(Vec<u8>, String)> = self
            .conn
            .query_row(
                "SELECT embedding, integrity_checksum FROM embeddings \
                 WHERE model_id = ?1 AND model_version = ?2 AND content_hash = ?3",
                params![model_id, model_version, content_hash],
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;

        let Some((bytes, recorded)) = row else {
            return Ok(None);
        };

        let recomputed = integrity_checksum(model_id, model_version, content_hash, &bytes);
        if recomputed != recorded {
            return Err(CacheError::IntegrityFailed {
                model_id: model_id.to_string(),
                model_version: model_version.to_string(),
                content_hash: content_hash.to_string(),
            });
        }

        Ok(Some(bytes_to_vec_f32(&bytes)))
    }

    /// Insert (or replace) a cached embedding.
    pub fn insert(
        &self,
        model_id: &str,
        model_version: &str,
        content_hash: &str,
        embedding: &[f32],
    ) -> Result<(), CacheError> {
        let bytes = vec_f32_to_bytes(embedding);
        let checksum = integrity_checksum(model_id, model_version, content_hash, &bytes);
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO embeddings \
                (model_id, model_version, content_hash, embedding, integrity_checksum, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(model_id, model_version, content_hash) DO UPDATE SET \
                embedding = excluded.embedding, \
                integrity_checksum = excluded.integrity_checksum, \
                created_at = excluded.created_at",
            params![model_id, model_version, content_hash, bytes, checksum, now],
        )?;
        Ok(())
    }

    /// Walk every row, recompute its checksum, report mismatches.
    pub fn verify_integrity(&self) -> Result<IntegrityReport, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT model_id, model_version, content_hash, embedding, integrity_checksum \
             FROM embeddings",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;

        let mut report = IntegrityReport::default();
        for row in rows {
            let (model_id, model_version, content_hash, bytes, recorded) = row?;
            report.scanned += 1;
            let recomputed = integrity_checksum(&model_id, &model_version, &content_hash, &bytes);
            if recomputed != recorded {
                report.bad.push(IntegrityIssue {
                    model_id,
                    model_version,
                    content_hash,
                    recorded,
                    recomputed,
                });
            }
        }
        Ok(report)
    }

    /// Sample up to `n` rows for re-embed drift detection (ADR-0007 §
    /// "Integrity verification"). Ordering is `RANDOM()` so repeated calls
    /// touch different rows.
    ///
    /// Returns the cached vectors. The caller is expected to look up the
    /// corresponding source text (cache only stores hashes), re-embed with
    /// the live [`crate::Embedder`], and compare. See
    /// [`crate::EmbedService::detect_drift`].
    pub fn sample_for_reembed(&self, n: usize) -> Result<Vec<SampleRow>, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT model_id, model_version, content_hash, embedding \
             FROM embeddings ORDER BY RANDOM() LIMIT ?1",
        )?;
        let cap = i64::try_from(n).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![cap], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut out = Vec::with_capacity(n);
        for row in rows {
            let (model_id, model_version, content_hash, bytes) = row?;
            out.push(SampleRow {
                model_id,
                model_version,
                content_hash,
                embedding: bytes_to_vec_f32(&bytes),
            });
        }
        Ok(out)
    }

    /// Total rows in the cache. Mostly useful in tests.
    pub fn len(&self) -> Result<usize, CacheError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
        usize::try_from(n)
            .map_err(|_| CacheError::Sqlite(rusqlite::Error::IntegralValueOutOfRange(0, n)))
    }

    /// True iff the cache has no rows.
    pub fn is_empty(&self) -> Result<bool, CacheError> {
        Ok(self.len()? == 0)
    }

    /// Test-only helper: corrupt the embedding bytes of a row without
    /// updating its checksum. Used by integrity tests.
    #[doc(hidden)]
    pub fn corrupt_for_test(
        &self,
        model_id: &str,
        model_version: &str,
        content_hash: &str,
    ) -> Result<(), CacheError> {
        self.conn.execute(
            "UPDATE embeddings SET embedding = X'00' \
             WHERE model_id = ?1 AND model_version = ?2 AND content_hash = ?3",
            params![model_id, model_version, content_hash],
        )?;
        Ok(())
    }

    /// Test-only helper: overwrite the embedding bytes **and** recompute the
    /// checksum so the row stays "valid" from the integrity-check standpoint
    /// but no longer matches what a live embedder would now produce.
    /// Simulates silent model drift (ADR-0007 §"Integrity verification" —
    /// the case re-embed sampling exists to catch).
    #[doc(hidden)]
    pub fn drift_for_test(
        &self,
        model_id: &str,
        model_version: &str,
        content_hash: &str,
        new_embedding: &[f32],
    ) -> Result<(), CacheError> {
        let bytes = vec_f32_to_bytes(new_embedding);
        let checksum = integrity_checksum(model_id, model_version, content_hash, &bytes);
        self.conn.execute(
            "UPDATE embeddings SET embedding = ?4, integrity_checksum = ?5 \
             WHERE model_id = ?1 AND model_version = ?2 AND content_hash = ?3",
            params![model_id, model_version, content_hash, bytes, checksum],
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Machine-local path resolution (ADR-0007)
// ---------------------------------------------------------------------------

/// Resolve the **machine-local** cache directory for the repo at
/// `workspace_root`.
///
/// Path is `<cache_home>/firetrail/<repo-hash>` where `<cache_home>` is
/// `$FIRETRAIL_CACHE_HOME` when set (used by tests to avoid polluting
/// `$HOME`), else `$HOME/.cache`. `<repo-hash>` is the first 16 hex
/// characters of `BLAKE3(repo_identity(workspace_root))` — sufficient
/// collision resistance for the repo-identity scope.
pub fn repo_cache_dir(workspace_root: &Path) -> Result<PathBuf, CacheError> {
    let base = cache_home_from_env()?;
    Ok(repo_cache_dir_under(&base, workspace_root))
}

/// Pure-function version of [`repo_cache_dir`] taking an explicit cache-home
/// base. Used internally and in tests so we can exercise path derivation
/// without mutating process-wide environment variables.
#[must_use]
pub fn repo_cache_dir_under(cache_home: &Path, workspace_root: &Path) -> PathBuf {
    let identity = repo_identity(workspace_root);
    let hex_hash = hex::encode(blake3::hash(identity.as_bytes()).as_bytes());
    let short = &hex_hash[..16];
    cache_home.join("firetrail").join(short)
}

fn cache_home_from_env() -> Result<PathBuf, CacheError> {
    if let Some(over) = std::env::var_os("FIRETRAIL_CACHE_HOME") {
        return Ok(PathBuf::from(over));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        CacheError::NoHome("$HOME is unset; set $FIRETRAIL_CACHE_HOME to override".to_string())
    })?;
    Ok(PathBuf::from(home).join(".cache"))
}

/// Stable repo identity string used to derive the cache path.
///
/// Resolution order:
/// 1. `git -C <root> config --get remote.origin.url` when present and
///    non-empty. This makes the cache shared across worktrees and clones of
///    the same logical repo.
/// 2. Canonicalised `workspace_root` as a fallback when there is no origin
///    remote (e.g. brand-new repos, scratch repos in tests).
#[must_use]
pub fn repo_identity(workspace_root: &Path) -> String {
    if let Some(url) = origin_url(workspace_root) {
        return url;
    }
    workspace_root.canonicalize().map_or_else(
        |_| workspace_root.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().into_owned(),
    )
}

fn origin_url(workspace_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ---------------------------------------------------------------------------
// SQLite plumbing
// ---------------------------------------------------------------------------

fn apply_pragmas(conn: &Connection) -> Result<(), CacheError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    Ok(())
}

fn init_schema(conn: &Connection) -> Result<(), CacheError> {
    let current: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < SCHEMA_VERSION {
        // ADR-0007 drop-and-recompute: pre-M3 caches contain only mock
        // vectors; re-embedding is cheap and lets us avoid carrying
        // schema-migration code for vectors that don't matter.
        conn.execute("DROP TABLE IF EXISTS embeddings", [])?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS embeddings (
            model_id            TEXT NOT NULL,
            model_version       TEXT NOT NULL,
            content_hash        TEXT NOT NULL,
            embedding           BLOB NOT NULL,
            integrity_checksum  TEXT NOT NULL,
            created_at          TEXT NOT NULL,
            PRIMARY KEY (model_id, model_version, content_hash)
        );",
    )?;
    Ok(())
}

fn vec_f32_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn bytes_to_vec_f32(b: &[u8]) -> Vec<f32> {
    let n = b.len() / 4;
    let mut out = Vec::with_capacity(n);
    for chunk in b.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().expect("chunks_exact(4) yields 4 bytes");
        out.push(f32::from_le_bytes(arr));
    }
    out
}

/// Compute the integrity checksum for a cache row.
///
/// Defined as
/// `BLAKE3(model_id || 0x00 || model_version || 0x00 || content_hash || 0x00 || embedding_bytes)`,
/// hex-encoded. The 0x00 separators eliminate the (theoretical) ambiguity
/// between concatenations of distinct (id, version, hash) tuples.
fn integrity_checksum(
    model_id: &str,
    model_version: &str,
    content_hash: &str,
    embedding_bytes: &[u8],
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(model_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(model_version.as_bytes());
    hasher.update(&[0]);
    hasher.update(content_hash.as_bytes());
    hasher.update(&[0]);
    hasher.update(embedding_bytes);
    hex::encode(hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_tmp() -> (tempfile::TempDir, EmbeddingCache) {
        let dir = tempdir().unwrap();
        let cache = EmbeddingCache::open(dir.path().join("e.db")).unwrap();
        (dir, cache)
    }

    #[test]
    fn round_trip_insert_and_lookup() {
        let (_dir, cache) = open_tmp();
        let v = vec![1.0_f32, 2.0, 3.0, -0.5];
        cache.insert("m1", "1", "abc123", &v).unwrap();
        let got = cache.lookup("m1", "1", "abc123").unwrap();
        assert_eq!(got, Some(v));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let (_dir, cache) = open_tmp();
        assert!(cache.lookup("m1", "1", "missing").unwrap().is_none());
    }

    #[test]
    fn upsert_replaces_existing_row() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h", &[1.0, 2.0]).unwrap();
        cache.insert("m", "1", "h", &[9.0, 8.0]).unwrap();
        assert_eq!(cache.lookup("m", "1", "h").unwrap(), Some(vec![9.0, 8.0]));
        assert_eq!(cache.len().unwrap(), 1);
    }

    #[test]
    fn model_version_partitions_rows() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h", &[1.0, 2.0]).unwrap();
        cache.insert("m", "2", "h", &[7.0, 7.0]).unwrap();
        // Different versions live in different partitions; both visible.
        assert_eq!(cache.lookup("m", "1", "h").unwrap(), Some(vec![1.0, 2.0]));
        assert_eq!(cache.lookup("m", "2", "h").unwrap(), Some(vec![7.0, 7.0]));
        assert_eq!(cache.len().unwrap(), 2);
    }

    #[test]
    fn verify_integrity_clean_cache() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h1", &[1.0]).unwrap();
        cache.insert("m", "1", "h2", &[2.0]).unwrap();
        let r = cache.verify_integrity().unwrap();
        assert_eq!(r.scanned, 2);
        assert!(r.bad.is_empty());
    }

    #[test]
    fn verify_integrity_catches_corruption() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h", &[1.0, 2.0]).unwrap();
        cache.corrupt_for_test("m", "1", "h").unwrap();
        let r = cache.verify_integrity().unwrap();
        assert_eq!(r.scanned, 1);
        assert_eq!(r.bad.len(), 1);
        assert_eq!(r.bad[0].model_id, "m");
        assert_eq!(r.bad[0].model_version, "1");
        assert_eq!(r.bad[0].content_hash, "h");
    }

    #[test]
    fn lookup_returns_integrity_failed_on_corrupted_row() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h", &[1.0, 2.0]).unwrap();
        cache.corrupt_for_test("m", "1", "h").unwrap();
        match cache.lookup("m", "1", "h") {
            Err(CacheError::IntegrityFailed {
                model_id,
                model_version,
                content_hash,
            }) => {
                assert_eq!(model_id, "m");
                assert_eq!(model_version, "1");
                assert_eq!(content_hash, "h");
            }
            other => panic!("expected IntegrityFailed, got {other:?}"),
        }
    }

    #[test]
    fn sample_for_reembed_returns_partition_keys_and_vectors() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h1", &[1.0, 2.0]).unwrap();
        cache.insert("m", "1", "h2", &[3.0, 4.0]).unwrap();
        cache.insert("m", "1", "h3", &[5.0, 6.0]).unwrap();
        let s = cache.sample_for_reembed(2).unwrap();
        assert_eq!(s.len(), 2);
        for row in s {
            assert_eq!(row.model_id, "m");
            assert_eq!(row.model_version, "1");
            assert_eq!(row.embedding.len(), 2);
        }
    }

    #[test]
    fn drift_for_test_keeps_integrity_valid() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "1", "h", &[1.0, 0.0]).unwrap();
        cache.drift_for_test("m", "1", "h", &[0.0, 1.0]).unwrap();
        // Integrity check still passes (drift is undetectable from
        // checksums alone — the whole point of sample-and-reembed).
        assert!(cache.verify_integrity().unwrap().bad.is_empty());
        // The cached vector is now the drifted one.
        assert_eq!(cache.lookup("m", "1", "h").unwrap(), Some(vec![0.0, 1.0]));
    }

    #[test]
    fn repo_cache_dir_under_uses_supplied_base() {
        let dir = tempdir().unwrap();
        let p = repo_cache_dir_under(dir.path(), dir.path());
        assert!(
            p.starts_with(dir.path().join("firetrail")),
            "{p:?} did not start with the supplied cache-home base"
        );
        // 16-char hash segment.
        let last = p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(last.len(), 16);
        assert!(last.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn repo_cache_dir_under_is_stable_for_same_identity() {
        let dir = tempdir().unwrap();
        let base = tempdir().unwrap();
        let a = repo_cache_dir_under(base.path(), dir.path());
        let b = repo_cache_dir_under(base.path(), dir.path());
        assert_eq!(a, b);
    }

    #[test]
    fn repo_identity_falls_back_to_canonical_path_when_no_origin() {
        let dir = tempdir().unwrap();
        // No git init → no origin → falls back to the canonical path.
        let id = repo_identity(dir.path());
        let canon = dir.path().canonicalize().unwrap();
        assert_eq!(id, canon.to_string_lossy());
    }
}
