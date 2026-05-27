//! FTS5 and vec0 virtual-table bootstrap for `ft-search`.
//!
//! These tables live in the same `SQLite` database that `ft-index` writes; they
//! are additive and idempotent so a fresh checkout of an existing repository
//! upgrades cleanly on the next `firetrail search` invocation.

use rusqlite::Connection;

use crate::error::SearchError;

/// FTS5 virtual table holding the searchable text for every record.
///
/// `content=''` keeps the index *external-content* — the FTS rowid is the
/// record id (string) stored in column `id`, but FTS5 does not own the source
/// text. We re-insert on every upsert; cleanup happens via explicit DELETE.
///
/// Tokenizer: `unicode61 remove_diacritics 2` — Unicode-aware folding with
/// diacritic stripping (so "café" matches "cafe").
const FTS_TABLE: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS records_fts USING fts5(
    id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);
";

/// vec0 virtual table holding 384-d embeddings.
///
/// `id_str` is the canonical record id (the same string stored in
/// `records.id`); `embedding` is the vector. sqlite-vec exposes vectors as
/// the column type `float[N]`.
const VEC_TABLE: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS records_vec USING vec0(
    id_str TEXT PRIMARY KEY,
    embedding float[384]
);
";

/// Ensure the FTS5 virtual table exists. Always runs.
pub fn ensure_fts(conn: &Connection) -> Result<(), SearchError> {
    conn.execute_batch(FTS_TABLE)?;
    Ok(())
}

/// Ensure the vec0 virtual table exists. Caller must have already loaded the
/// `sqlite-vec` extension; on builds where the extension is unavailable this
/// function is never called.
#[cfg(feature = "sqlite-vec")]
pub fn ensure_vec(conn: &Connection) -> Result<(), SearchError> {
    conn.execute_batch(VEC_TABLE)?;
    Ok(())
}

#[cfg(not(feature = "sqlite-vec"))]
#[allow(dead_code, clippy::unnecessary_wraps)]
pub fn ensure_vec(_conn: &Connection) -> Result<(), SearchError> {
    // Never called when the feature is off; kept for symmetry so callers don't
    // need a second `cfg` gate.
    Ok(())
}

/// Used only to silence dead-code lints on `VEC_TABLE` when the feature is
/// off (the constant is still referenced by `ensure_vec` in the on path).
#[cfg(not(feature = "sqlite-vec"))]
#[allow(dead_code)]
const _UNUSED_VEC_TABLE: &str = VEC_TABLE;
