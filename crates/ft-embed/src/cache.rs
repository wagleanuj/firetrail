//! SQLite-backed content-hash embedding cache.
//!
//! Rows are keyed by `(model_id, content_hash)` so switching worktrees, or
//! re-importing unchanged source files, doesn't re-embed (ADR-0007).
//!
//! Each row carries an `integrity_checksum` — a BLAKE3 hex digest of
//! `(model_id || content_hash || embedding_bytes)`. [`EmbeddingCache::lookup`]
//! validates the checksum on read; [`EmbeddingCache::verify_integrity`]
//! sweeps the entire table.
//!
//! The on-disk file lives at `<workspace_root>/.firetrail/cache/embeddings.db`
//! when constructed via [`EmbeddingCache::open_under`].

use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

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
    #[error("integrity check failed for ({model_id}, {content_hash})")]
    IntegrityFailed {
        /// Model id of the corrupt row.
        model_id: String,
        /// Content hash of the corrupt row.
        content_hash: String,
    },
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
    /// Content hash of the offending row.
    pub content_hash: String,
    /// Checksum recorded in the row.
    pub recorded: String,
    /// Checksum recomputed from row contents.
    pub recomputed: String,
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

    /// Convenience: open the cache under `<workspace_root>/.firetrail/cache/embeddings.db`.
    pub fn open_under(workspace_root: impl AsRef<Path>) -> Result<Self, CacheError> {
        let path = workspace_root
            .as_ref()
            .join(".firetrail")
            .join("cache")
            .join("embeddings.db");
        Self::open(path)
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
        content_hash: &str,
    ) -> Result<Option<Vec<f32>>, CacheError> {
        let row: Option<(Vec<u8>, String)> = self
            .conn
            .query_row(
                "SELECT embedding, integrity_checksum FROM embeddings \
                 WHERE model_id = ?1 AND content_hash = ?2",
                params![model_id, content_hash],
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;

        let Some((bytes, recorded)) = row else {
            return Ok(None);
        };

        let recomputed = integrity_checksum(model_id, content_hash, &bytes);
        if recomputed != recorded {
            return Err(CacheError::IntegrityFailed {
                model_id: model_id.to_string(),
                content_hash: content_hash.to_string(),
            });
        }

        Ok(Some(bytes_to_vec_f32(&bytes)))
    }

    /// Insert (or replace) a cached embedding.
    pub fn insert(
        &self,
        model_id: &str,
        content_hash: &str,
        embedding: &[f32],
    ) -> Result<(), CacheError> {
        let bytes = vec_f32_to_bytes(embedding);
        let checksum = integrity_checksum(model_id, content_hash, &bytes);
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO embeddings \
                (model_id, content_hash, embedding, integrity_checksum, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(model_id, content_hash) DO UPDATE SET \
                embedding = excluded.embedding, \
                integrity_checksum = excluded.integrity_checksum, \
                created_at = excluded.created_at",
            params![model_id, content_hash, bytes, checksum, now],
        )?;
        Ok(())
    }

    /// Walk every row, recompute its checksum, report mismatches.
    pub fn verify_integrity(&self) -> Result<IntegrityReport, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT model_id, content_hash, embedding, integrity_checksum FROM embeddings",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;

        let mut report = IntegrityReport::default();
        for row in rows {
            let (model_id, content_hash, bytes, recorded) = row?;
            report.scanned += 1;
            let recomputed = integrity_checksum(&model_id, &content_hash, &bytes);
            if recomputed != recorded {
                report.bad.push(IntegrityIssue {
                    model_id,
                    content_hash,
                    recorded,
                    recomputed,
                });
            }
        }
        Ok(report)
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
    pub fn corrupt_for_test(&self, model_id: &str, content_hash: &str) -> Result<(), CacheError> {
        self.conn.execute(
            "UPDATE embeddings SET embedding = X'00' \
             WHERE model_id = ?1 AND content_hash = ?2",
            params![model_id, content_hash],
        )?;
        Ok(())
    }
}

fn apply_pragmas(conn: &Connection) -> Result<(), CacheError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    Ok(())
}

fn init_schema(conn: &Connection) -> Result<(), CacheError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS embeddings (
            model_id            TEXT NOT NULL,
            content_hash        TEXT NOT NULL,
            embedding           BLOB NOT NULL,
            integrity_checksum  TEXT NOT NULL,
            created_at          TEXT NOT NULL,
            PRIMARY KEY (model_id, content_hash)
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
    // Tolerate trailing partial bytes by truncating; well-formed rows always
    // have len % 4 == 0.
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
/// Defined as `BLAKE3(model_id || 0x00 || content_hash || 0x00 || embedding_bytes)`,
/// hex-encoded. The 0x00 separators eliminate the (theoretical) ambiguity
/// between e.g. `("ab", "cd")` and `("a", "bcd")`.
fn integrity_checksum(model_id: &str, content_hash: &str, embedding_bytes: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(model_id.as_bytes());
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
        cache.insert("m1", "abc123", &v).unwrap();
        let got = cache.lookup("m1", "abc123").unwrap();
        assert_eq!(got, Some(v));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let (_dir, cache) = open_tmp();
        assert!(cache.lookup("m1", "missing").unwrap().is_none());
    }

    #[test]
    fn upsert_replaces_existing_row() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "h", &[1.0, 2.0]).unwrap();
        cache.insert("m", "h", &[9.0, 8.0]).unwrap();
        assert_eq!(cache.lookup("m", "h").unwrap(), Some(vec![9.0, 8.0]));
        assert_eq!(cache.len().unwrap(), 1);
    }

    #[test]
    fn verify_integrity_clean_cache() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "h1", &[1.0]).unwrap();
        cache.insert("m", "h2", &[2.0]).unwrap();
        let r = cache.verify_integrity().unwrap();
        assert_eq!(r.scanned, 2);
        assert!(r.bad.is_empty());
    }

    #[test]
    fn verify_integrity_catches_corruption() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "h", &[1.0, 2.0]).unwrap();
        cache.corrupt_for_test("m", "h").unwrap();
        let r = cache.verify_integrity().unwrap();
        assert_eq!(r.scanned, 1);
        assert_eq!(r.bad.len(), 1);
        assert_eq!(r.bad[0].model_id, "m");
        assert_eq!(r.bad[0].content_hash, "h");
    }

    #[test]
    fn lookup_returns_integrity_failed_on_corrupted_row() {
        let (_dir, cache) = open_tmp();
        cache.insert("m", "h", &[1.0, 2.0]).unwrap();
        cache.corrupt_for_test("m", "h").unwrap();
        match cache.lookup("m", "h") {
            Err(CacheError::IntegrityFailed {
                model_id,
                content_hash,
            }) => {
                assert_eq!(model_id, "m");
                assert_eq!(content_hash, "h");
            }
            other => panic!("expected IntegrityFailed, got {other:?}"),
        }
    }
}
