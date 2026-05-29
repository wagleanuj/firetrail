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

/// Side table holding per-record search metadata that does not live on the
/// `records_fts` virtual table (FTS5 columns are tokenized text).
///
/// Today this carries the materialised `trust` state read from the record
/// body at upsert time (memory bodies carry a `TrustState` field; work-
/// tracking kinds default to `reviewed`). Search ranking, the
/// `--min-trust` filter, and the `--trust` filter all read this column so
/// trust transitions become visible to search without an index rebuild.
///
/// The `kind`, `title`, `updated_at`, and `owning_scope` columns allow
/// synthetic documents (scopes, identities, audit entries) that have no
/// corresponding `records` row to be found by search without a JOIN.
const META_TABLE: &str = "
CREATE TABLE IF NOT EXISTS records_search_meta (
    id TEXT PRIMARY KEY,
    trust TEXT NOT NULL,
    kind TEXT,
    title TEXT,
    updated_at TEXT,
    owning_scope TEXT
);
";

/// Columns added to `records_search_meta` after its original (id, trust)
/// shape. Added idempotently so existing databases upgrade in place.
const META_ADDED_COLUMNS: &[(&str, &str)] = &[
    ("kind", "TEXT"),
    ("title", "TEXT"),
    ("updated_at", "TEXT"),
    ("owning_scope", "TEXT"),
];

/// Ensure the FTS5 virtual table and side metadata table exist. Always runs.
pub fn ensure_fts(conn: &Connection) -> Result<(), SearchError> {
    conn.execute_batch(FTS_TABLE)?;
    conn.execute_batch(META_TABLE)?;
    migrate_meta_columns(conn)?;
    Ok(())
}

/// Add any missing `records_search_meta` columns. `ALTER TABLE ADD COLUMN` has
/// no `IF NOT EXISTS`, so we probe `PRAGMA table_info` first.
fn migrate_meta_columns(conn: &Connection) -> Result<(), SearchError> {
    let mut existing = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare("PRAGMA table_info(records_search_meta)")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        for name in rows {
            existing.insert(name?);
        }
    }
    for (name, ty) in META_ADDED_COLUMNS {
        if !existing.contains(*name) {
            conn.execute_batch(&format!(
                "ALTER TABLE records_search_meta ADD COLUMN {name} {ty};"
            ))?;
        }
    }
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
