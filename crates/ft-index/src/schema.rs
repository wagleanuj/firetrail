//! Schema bootstrap and migrations for the `SQLite` index.
//!
//! Migrations are version-numbered and applied in order. `Index::open` calls
//! [`apply_pending`]; a schema bumped past [`CURRENT_VERSION`] refuses to open
//! and asks the user to upgrade or rebuild.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::IndexError;

/// Schema version this build of `ft-index` understands.
pub const CURRENT_VERSION: u32 = 1;

/// SQL applied at version 1. Mirrors the spec exactly.
const V1_UP: &str = r"
CREATE TABLE IF NOT EXISTS schema_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS records (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    priority TEXT NOT NULL,
    owner TEXT,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    closed_at TEXT,
    owning_scope TEXT,
    state_hash TEXT NOT NULL,
    file_path TEXT NOT NULL,
    file_mtime INTEGER NOT NULL,
    origin TEXT NOT NULL,
    parent_id TEXT
);
CREATE INDEX IF NOT EXISTS records_kind_status ON records(kind, status);
CREATE INDEX IF NOT EXISTS records_owner ON records(owner);
CREATE INDEX IF NOT EXISTS records_scope ON records(owning_scope);
CREATE INDEX IF NOT EXISTS records_updated_at ON records(updated_at);
CREATE INDEX IF NOT EXISTS records_parent ON records(parent_id);

CREATE TABLE IF NOT EXISTS labels (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (record_id, key, value)
);
CREATE INDEX IF NOT EXISTS labels_key_value ON labels(key, value);

CREATE TABLE IF NOT EXISTS affected_scopes (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    PRIMARY KEY (record_id, scope)
);

CREATE TABLE IF NOT EXISTS applies_to (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    glob TEXT NOT NULL,
    PRIMARY KEY (record_id, glob)
);

CREATE TABLE IF NOT EXISTS relations (
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL,
    created_by TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id, kind)
);
CREATE INDEX IF NOT EXISTS relations_to ON relations(to_id, kind);
CREATE INDEX IF NOT EXISTS relations_from ON relations(from_id, kind);

CREATE TABLE IF NOT EXISTS claims (
    record_id TEXT PRIMARY KEY REFERENCES records(id) ON DELETE CASCADE,
    claimed_by TEXT NOT NULL,
    claimed_at TEXT NOT NULL,
    claim_source TEXT NOT NULL,
    claim_expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS acceptance_criteria (
    id TEXT NOT NULL,
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    status TEXT NOT NULL,
    evidence_url TEXT,
    checked_by TEXT,
    checked_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    proposed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (record_id, id)
);

CREATE TABLE IF NOT EXISTS evidence (
    id TEXT NOT NULL,
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    url TEXT NOT NULL,
    description TEXT,
    created_at TEXT NOT NULL,
    created_by TEXT NOT NULL,
    commit_sha TEXT,
    symbol_name TEXT,
    content_hash TEXT,
    PRIMARY KEY (record_id, id)
);
";

/// Apply session-wide PRAGMAs. Called once per connection open.
pub fn apply_pragmas(conn: &Connection) -> Result<(), IndexError> {
    // WAL: concurrent readers + single writer without blocking.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL;", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

/// Apply every pending migration from the recorded version up to
/// [`CURRENT_VERSION`].
///
/// Returns [`IndexError::Migration`] if the on-disk version is newer than what
/// this build supports.
pub fn apply_pending(conn: &mut Connection) -> Result<(), IndexError> {
    // Ensure schema_meta exists before reading it.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;

    let on_disk: u32 = read_version(conn)?;
    if on_disk > CURRENT_VERSION {
        return Err(IndexError::Migration(format!(
            "on-disk schema version {on_disk} is newer than supported {CURRENT_VERSION}; \
             upgrade `firetrail` or run `firetrail index rebuild`",
        )));
    }
    if on_disk == CURRENT_VERSION {
        return Ok(());
    }

    let tx = conn.transaction()?;
    // Future: loop over a Vec<(version, sql)> and apply each missing version.
    if on_disk < 1 {
        tx.execute_batch(V1_UP)
            .map_err(|e| IndexError::Migration(format!("v1: {e}")))?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO schema_meta(key, value) VALUES('schema_version', ?1);",
        params![CURRENT_VERSION.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Read the recorded schema version, returning 0 if unset.
pub fn read_version(conn: &Connection) -> Result<u32, IndexError> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.and_then(|s| s.parse::<u32>().ok()).unwrap_or(0))
}
