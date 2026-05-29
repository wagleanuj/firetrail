# Index scopes, identities & audit in ft-search — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make scopes, identities, and per-entry audit history searchable in `ft-search` via synthetic documents (lexical + vector), exposed through `firetrail search --kind scope|identity|audit`.

**Architecture:** Introduce search-layer `DocId` / `IndexKind` types broader than `RecordId` / `RecordKind`. Decouple search-hit metadata from the relational `records` table by making `records_search_meta` self-sufficient. Add a generalized `upsert_document` entry point; wire scopes/identities/audit into the `index rebuild`/`refresh` pass (lexical + best-effort embedding via the existing daemon path).

**Tech Stack:** Rust, rusqlite (SQLite FTS5 + sqlite-vec), the ft-* workspace crates (ft-search, ft-core, ft-scope, ft-identity, ft-embed, ft-cli, ft-ops).

**Spec:** `docs/superpowers/specs/2026-05-29-index-missing-domains-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/ft-search/src/kind.rs` (**new**) | `IndexKind`, `DocId` types + parsing/serialization |
| `crates/ft-search/src/lib.rs` | export new module + types |
| `crates/ft-search/src/schema.rs` | add 4 columns to `records_search_meta` (idempotent migration) |
| `crates/ft-search/src/hit.rs` | `SearchHit.id: DocId`, `kind: IndexKind` |
| `crates/ft-search/src/query.rs` | `SearchQuery.kind_filter: Vec<IndexKind>` |
| `crates/ft-search/src/engine.rs` | `IndexDoc`, `upsert_document`, self-sufficient `lookup_meta`, `DocId`-typed `upsert_vector`/`delete`, `IndexKind` plumbing |
| `crates/ft-search/src/sources.rs` (**new**) | pure `scope_doc` / `identity_docs` / `audit_docs` builders |
| `crates/ft-cli/src/commands/index_cmd.rs` | reindex scopes/identities/audit + dispatch embedding |
| `crates/ft-cli/src/commands/daemon_cmd.rs` | `SearchEngineIndexer` accepts `DocId` strings |
| `crates/ft-cli/src/cli.rs` | new `SearchKindArg` (13 variants) |
| `crates/ft-cli/src/commands/search.rs` | `From<SearchHit>` + quarantine fix + `--kind` mapping |
| `crates/ft-ops/src/memory/search.rs` | `From<SearchHit>` + quarantine fix + `MemoryKind::to_index_kind` |

---

## Task 1: `IndexKind` and `DocId` types

**Files:**
- Create: `crates/ft-search/src/kind.rs`
- Modify: `crates/ft-search/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/ft-search/src/kind.rs`:

```rust
//! Search-layer identity (`DocId`) and kind (`IndexKind`) types.
//!
//! Search indexes more than `ft_core::Record`s: it also indexes synthetic
//! documents for scopes, identities, and per-entry audit history. Those have
//! no `RecordId` (which requires a 64-hex tail, ADR-0015) and no `RecordKind`.
//! These types widen the search surface to cover both.

use ft_core::{RecordId, RecordKind};
use serde::{Serialize, Serializer};

/// Search-layer kind: the record kinds plus the synthetic domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexKind {
    /// One of the ten `ft_core::RecordKind`s.
    Record(RecordKind),
    /// A scope definition (`.firetrail/scopes.yaml`).
    Scope,
    /// A registered identity (`.firetrail/identities.yaml`).
    Identity,
    /// One audit/history entry of a record.
    Audit,
}

/// Search-layer document id. Records keep their `RecordId`; synthetic docs use
/// a namespaced key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocId {
    /// A real record.
    Record(RecordId),
    /// A synthetic document. `key` is domain-specific (scope id, identity id,
    /// or `<record-id>#h<n>`).
    Synthetic {
        /// Which synthetic domain this id belongs to.
        kind: IndexKind,
        /// The domain-specific key.
        key: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rid() -> RecordId {
        RecordId::from_string(format!("TASK-{}", "a".repeat(64))).unwrap()
    }

    #[test]
    fn index_kind_label_roundtrips() {
        assert_eq!(IndexKind::Scope.label(), "scope");
        assert_eq!(IndexKind::Record(RecordKind::Task).label(), "task");
        assert_eq!(IndexKind::parse_label("identity"), Some(IndexKind::Identity));
        assert_eq!(
            IndexKind::parse_label("epic"),
            Some(IndexKind::Record(RecordKind::Epic))
        );
        assert_eq!(IndexKind::parse_label("nope"), None);
    }

    #[test]
    fn docid_record_storage_str_is_bare_recordid() {
        let d = DocId::Record(rid());
        assert_eq!(d.as_storage_str(), rid().as_str());
        assert_eq!(DocId::parse(&d.as_storage_str()), d);
        assert_eq!(d.as_record_id(), Some(&rid()));
    }

    #[test]
    fn docid_synthetic_storage_str_is_tagged() {
        let d = DocId::Synthetic {
            kind: IndexKind::Scope,
            key: "apps/checkout".to_string(),
        };
        assert_eq!(d.as_storage_str(), "scope:apps/checkout");
        assert_eq!(DocId::parse("scope:apps/checkout"), d);
        assert_eq!(d.as_record_id(), None);
    }

    #[test]
    fn docid_audit_key_embeds_recordid() {
        let key = format!("{}#h3", rid().as_str());
        let d = DocId::Synthetic { kind: IndexKind::Audit, key: key.clone() };
        assert_eq!(d.as_storage_str(), format!("audit:{key}"));
        assert_eq!(DocId::parse(&format!("audit:{key}")), d);
    }

    #[test]
    fn index_kind_serializes_lowercase() {
        let j = serde_json::to_string(&IndexKind::Scope).unwrap();
        assert_eq!(j, "\"scope\"");
        let j = serde_json::to_string(&IndexKind::Record(RecordKind::Bug)).unwrap();
        assert_eq!(j, "\"bug\"");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ft-search --lib kind::`
Expected: FAIL — `label`, `parse_label`, `as_storage_str`, `parse`, `as_record_id` not defined.

- [ ] **Step 3: Implement the types**

Add to `crates/ft-search/src/kind.rs` (above the `#[cfg(test)]` module):

```rust
impl IndexKind {
    /// Stable lowercase label (matches `RecordKind`'s serde labels for the
    /// record variants).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            IndexKind::Record(k) => record_kind_label(k),
            IndexKind::Scope => "scope",
            IndexKind::Identity => "identity",
            IndexKind::Audit => "audit",
        }
    }

    /// Inverse of [`Self::label`]. Returns `None` for unknown labels.
    #[must_use]
    pub fn parse_label(s: &str) -> Option<Self> {
        Some(match s {
            "scope" => IndexKind::Scope,
            "identity" => IndexKind::Identity,
            "audit" => IndexKind::Audit,
            other => IndexKind::Record(record_kind_from_label(other)?),
        })
    }
}

impl Serialize for IndexKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.label())
    }
}

impl DocId {
    /// The canonical string form used as the FTS / vec primary key.
    ///
    /// - `Record` → bare `RecordId` string (`TASK-<64hex>`).
    /// - `Synthetic` → `<tag>:<key>` where tag ∈ {scope, identity, audit}.
    #[must_use]
    pub fn as_storage_str(&self) -> String {
        match self {
            DocId::Record(id) => id.as_str().to_string(),
            DocId::Synthetic { kind, key } => format!("{}:{}", kind.label(), key),
        }
    }

    /// Parse the storage form. A string that is a valid `RecordId` → `Record`;
    /// a `<tag>:<key>` string → `Synthetic`. Anything else falls back to a
    /// `Record`-parse attempt and, failing that, an `Audit` synthetic (so an
    /// unknown id never panics — it just won't resolve metadata).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        if let Ok(id) = RecordId::from_string(s.to_string()) {
            return DocId::Record(id);
        }
        if let Some((tag, key)) = s.split_once(':') {
            if let Some(kind) = IndexKind::parse_label(tag) {
                return DocId::Synthetic { kind, key: key.to_string() };
            }
        }
        DocId::Synthetic { kind: IndexKind::Audit, key: s.to_string() }
    }

    /// The backing `RecordId`, if this doc is a real record. Synthetic docs
    /// return `None` (used to skip record-only operations like quarantine).
    #[must_use]
    pub fn as_record_id(&self) -> Option<&RecordId> {
        match self {
            DocId::Record(id) => Some(id),
            DocId::Synthetic { .. } => None,
        }
    }
}

impl From<RecordId> for DocId {
    fn from(id: RecordId) -> Self {
        DocId::Record(id)
    }
}

fn record_kind_label(k: RecordKind) -> &'static str {
    match k {
        RecordKind::Epic => "epic",
        RecordKind::Task => "task",
        RecordKind::Subtask => "subtask",
        RecordKind::Bug => "bug",
        RecordKind::Incident => "incident",
        RecordKind::Finding => "finding",
        RecordKind::Runbook => "runbook",
        RecordKind::Decision => "decision",
        RecordKind::Gotcha => "gotcha",
        RecordKind::Memory => "memory",
    }
}

fn record_kind_from_label(s: &str) -> Option<RecordKind> {
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
```

Add to `crates/ft-search/src/lib.rs` after `mod hit;`:

```rust
mod kind;
```

And to the `pub use` block:

```rust
pub use kind::{DocId, IndexKind};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ft-search --lib kind::`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/ft-search/src/kind.rs crates/ft-search/src/lib.rs
git commit -m "feat(ft-search): add DocId and IndexKind search-layer types"
```

---

## Task 2: Self-sufficient `records_search_meta` schema

**Files:**
- Modify: `crates/ft-search/src/schema.rs`
- Test: `crates/ft-search/tests/integration.rs` (new test)

- [ ] **Step 1: Write the failing test**

Add to `crates/ft-search/tests/integration.rs`:

```rust
#[test]
fn meta_table_has_synthetic_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("index.db");
    let engine = ft_search::SearchEngine::open(&db).unwrap();
    engine.ensure_schema().unwrap();
    // Re-open to prove the migration is idempotent across connections.
    let engine2 = ft_search::SearchEngine::open(&db).unwrap();
    engine2.ensure_schema().unwrap();

    let cols = engine2.debug_meta_columns().unwrap();
    for expected in ["id", "trust", "kind", "title", "updated_at", "owning_scope"] {
        assert!(cols.contains(&expected.to_string()), "missing column {expected}");
    }
}
```

> Note: `debug_meta_columns` is a tiny test helper added in Step 3.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ft-search --test integration meta_table_has_synthetic_columns`
Expected: FAIL — `debug_meta_columns` not defined (and columns absent).

- [ ] **Step 3: Implement the migration**

In `crates/ft-search/src/schema.rs`, replace the `META_TABLE` constant body and `ensure_fts`:

```rust
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

/// Ensure the FTS5 virtual table and side metadata table exist, and that the
/// metadata table carries the synthetic-document columns. Always runs.
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
```

Add the test helper to `crates/ft-search/src/engine.rs` (inside `impl SearchEngine`):

```rust
    /// Test-only: list the column names of `records_search_meta`.
    #[doc(hidden)]
    pub fn debug_meta_columns(&self) -> Result<Vec<String>, SearchError> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(records_search_meta)")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p ft-search --test integration meta_table_has_synthetic_columns`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ft-search/src/schema.rs crates/ft-search/src/engine.rs crates/ft-search/tests/integration.rs
git commit -m "feat(ft-search): extend records_search_meta with synthetic-doc columns"
```

---

## Task 3: `IndexDoc` + `upsert_document` + self-sufficient lookups; switch `SearchHit`/`SearchQuery` to `DocId`/`IndexKind`

This is the core engine change. It is large but cohesive: after it, the engine speaks `DocId`/`IndexKind` end to end and can index documents that have no `records` row.

**Files:**
- Modify: `crates/ft-search/src/hit.rs`
- Modify: `crates/ft-search/src/query.rs`
- Modify: `crates/ft-search/src/engine.rs`
- Test: `crates/ft-search/tests/integration.rs`

- [ ] **Step 1: Switch the public types**

In `crates/ft-search/src/hit.rs`, change imports and `SearchHit`:

```rust
use ft_core::TrustState;

use crate::kind::{DocId, IndexKind};
```

```rust
pub struct SearchHit {
    /// Document id (record or synthetic).
    pub id: DocId,
    /// Document kind.
    pub kind: IndexKind,
    /// Short title.
    pub title: String,
    /// Final ranking score, after trust + recency multipliers. Higher = better.
    pub score: f32,
    /// Trust state at index time.
    pub trust: TrustState,
    /// Which signal produced this hit.
    pub mode: HitMode,
}
```

In `crates/ft-search/src/query.rs`, change the import and `kind_filter`:

```rust
use ft_core::TrustState;

use crate::kind::IndexKind;
```

```rust
    /// Restrict to these kinds. Empty means "all kinds".
    pub kind_filter: Vec<IndexKind>,
```

- [ ] **Step 2: Add `IndexDoc` + `upsert_document`; refactor `upsert_lexical`**

In `crates/ft-search/src/engine.rs`, add imports:

```rust
use crate::kind::{DocId, IndexKind};
```

Add the document type (top level, near `ScoringRow`):

```rust
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
```

Add `upsert_document` and refactor `upsert_lexical` (replace the existing `upsert_lexical`):

```rust
    /// Upsert a document's lexical row and full search metadata. The metadata
    /// is written self-sufficiently (kind/title/updated_at/owning_scope/trust)
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
        let (title, body) = record_to_text(record);
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
```

> Verify field paths while implementing: confirm `record.envelope.owning_scope` and `record.envelope.updated_at` exist (the `records` table has `owning_scope` and `updated_at`, and `lookup_meta` already reads both). If `owning_scope` lives elsewhere on the envelope, adapt the field access — do not invent a field.

- [ ] **Step 3: Make `lookup_meta` self-sufficient and `IndexKind`-typed**

Replace `RecordMeta` and `lookup_meta` in `crates/ft-search/src/engine.rs`:

```rust
#[derive(Debug, Clone)]
struct RecordMeta {
    kind: IndexKind,
    title: String,
    trust: TrustState,
    owning_scope: Option<String>,
    updated_at: DateTime<Utc>,
}
```

```rust
    fn lookup_meta(&self, id_str: &str) -> Result<Option<RecordMeta>, SearchError> {
        // Prefer the relational record row when present; otherwise fall back to
        // the self-sufficient side-table columns (synthetic docs).
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
        let kind_s = kind_s
            .ok_or_else(|| SearchError::Integrity("missing kind for indexed doc".into()))?;
        let kind = IndexKind::parse_label(&kind_s)
            .ok_or_else(|| SearchError::Integrity(format!("unknown kind `{kind_s}`")))?;
        let updated_at_s = updated_at_s
            .ok_or_else(|| SearchError::Integrity("missing updated_at".into()))?;
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
```

> The old query used `FROM records r LEFT JOIN records_search_meta m`. Synthetic docs have **no** `records` row, so the new query drives from `records_search_meta m LEFT JOIN records r`. Pre-existing records still have a `m` row (written by `upsert_document`), so they continue to resolve.

- [ ] **Step 4: Fix `search()` to build `DocId`; `default_trust_for_index_kind`; `filters_pass`**

In `search()`, replace the `RecordId::from_string(id_str.clone())` block:

```rust
            let (mode, score) = combine_score(&row, &meta, now, query.mode, self.vec_loaded);
            let doc_id = DocId::parse(&id_str);

            hits.push(SearchHit {
                id: doc_id,
                kind: meta.kind,
                title: meta.title,
                score,
                trust: meta.trust,
                mode,
            });
```

Do the same substitution in `similar()`'s lexical-fallback loop (replace its `RecordId::from_string(...)` + `SearchHit { id: record_id, ... }` with `DocId::parse(&row.id_str)`).

**Keep** the existing `default_trust_for_kind(RecordKind)` — `trust_for_record` still calls it, and `upsert_lexical` (Task 3 Step 2) relies on `trust_for_record` unchanged. **Add** an `IndexKind` variant used only by `lookup_meta`'s fallback:

```rust
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
```

`filters_pass` already compares `query.kind_filter.contains(&meta.kind)`; both are now `IndexKind`, so it compiles unchanged. Keep `trust_for_record`, `default_trust_for_kind`, and `record_to_text` as-is. The old `parse_kind` helper becomes unused once `lookup_meta` switches to `IndexKind::parse_label`; remove it only if the compiler flags it dead (other call sites may remain — check first).

- [ ] **Step 5: Update existing ft-search tests for the new types**

Existing assertions in `crates/ft-search/tests/integration.rs` compare `hit.id` to a `RecordId` and read `hit.kind` as `RecordKind`. Update them:

- `hit.id == task.envelope.id` → `hit.id == ft_search::DocId::Record(task.envelope.id.clone())` (or assert `hit.id.as_record_id() == Some(&task.envelope.id)`).
- `hit.kind == RecordKind::Task` → `hit.kind == ft_search::IndexKind::Record(ft_core::RecordKind::Task)`.
- Any `query.kind_filter = vec![RecordKind::X]` → `vec![ft_search::IndexKind::Record(ft_core::RecordKind::X)]`.

Make the minimal edits to compile; do not change test intent.

- [ ] **Step 6: Run the whole ft-search suite**

Run: `cargo test -p ft-search`
Expected: PASS (existing tests + Task 2 test). Fix compile errors in call sites the compiler points to.

- [ ] **Step 7: Commit**

```bash
git add crates/ft-search/src/hit.rs crates/ft-search/src/query.rs crates/ft-search/src/engine.rs crates/ft-search/tests/integration.rs
git commit -m "feat(ft-search): IndexDoc/upsert_document + DocId/IndexKind hits, self-sufficient meta"
```

---

## Task 4: Widen `upsert_vector` / `delete` to `DocId`; update indexer impls

**Files:**
- Modify: `crates/ft-search/src/engine.rs`
- Modify: `crates/ft-cli/src/commands/daemon_cmd.rs`
- Modify: `crates/ft-search/tests/integration.rs` (call-site updates)

- [ ] **Step 1: Change engine signatures**

In `crates/ft-search/src/engine.rs`, change `upsert_vector`, `delete`, and the `#[cfg]` inner helpers to take `&DocId` and key on `doc.as_storage_str()`:

```rust
    pub fn upsert_vector(&self, id: &DocId, embedding: &[f32]) -> Result<(), SearchError> {
        if embedding.len() != crate::EMBEDDING_DIM {
            return Err(SearchError::DimensionMismatch {
                expected: crate::EMBEDDING_DIM,
                actual: embedding.len(),
            });
        }
        if !self.vec_loaded {
            tracing::warn!(record = %id.as_storage_str(),
                "upsert_vector called but sqlite-vec is not loaded; skipping");
            return Ok(());
        }
        self.upsert_vector_inner(id, embedding)
    }

    pub fn delete(&self, id: &DocId) -> Result<(), SearchError> {
        let id_str = id.as_storage_str();
        self.conn.execute("DELETE FROM records_fts WHERE id = ?1", params![id_str])?;
        self.conn.execute("DELETE FROM records_search_meta WHERE id = ?1", params![id_str])?;
        if self.vec_loaded {
            self.conn.execute("DELETE FROM records_vec WHERE id_str = ?1", params![id_str])?;
        }
        Ok(())
    }
```

In the `#[cfg(feature = "sqlite-vec")] fn upsert_vector_inner`, change the signature to `id: &DocId` and use `let id_str = id.as_storage_str();`. Update the `#[cfg(not(...))]` stub signature to match.

- [ ] **Step 2: Update the daemon indexer to accept synthetic ids**

In `crates/ft-cli/src/commands/daemon_cmd.rs`, replace `SearchEngineIndexer::upsert_vector`:

```rust
impl RecordIndexer for SearchEngineIndexer {
    fn upsert_vector(&self, record_id: &str, embedding: &[f32]) -> Result<(), String> {
        // Accept both real record ids and synthetic doc ids (scope:/identity:/audit:).
        let id = ft_search::DocId::parse(record_id);
        let guard = self.engine.lock().map_err(|e| e.to_string())?;
        guard.upsert_vector(&id, embedding).map_err(|e| e.to_string())
    }
}
```

Remove the now-unused `RecordId` import if the compiler flags it.

- [ ] **Step 3: Update ft-search test call sites**

In `crates/ft-search/tests/integration.rs`, the `upsert_vector(&task.envelope.id, ...)` calls now need a `&DocId`. Update each:

```rust
engine.upsert_vector(&ft_search::DocId::Record(task.envelope.id.clone()), &vec).unwrap();
```

Apply to every `upsert_vector` / `delete` call site the compiler flags (the `one_hot` similarity tests near lines 399–402, the noop test near 306, the dimension-mismatch test near 317).

- [ ] **Step 4: Build + test**

Run: `cargo test -p ft-search && cargo build -p ft-cli`
Expected: PASS / clean build.

- [ ] **Step 5: Commit**

```bash
git add crates/ft-search/src/engine.rs crates/ft-cli/src/commands/daemon_cmd.rs crates/ft-search/tests/integration.rs
git commit -m "feat(ft-search): DocId-typed upsert_vector/delete; daemon indexer accepts synthetic ids"
```

---

## Task 5: Per-domain document builders (`sources.rs`)

**Files:**
- Create: `crates/ft-search/src/sources.rs`
- Modify: `crates/ft-search/src/lib.rs`
- Modify: `crates/ft-search/Cargo.toml` (add `ft-scope`, `ft-identity` dev/normal deps — see Step 3)

- [ ] **Step 1: Write the failing tests**

Create `crates/ft-search/src/sources.rs`:

```rust
//! Pure builders that lower scopes, identities, and audit entries into
//! [`IndexDoc`]s for indexing. Kept in ft-search so the engine owns the
//! title/body shape; the reindex command supplies the domain objects.

use chrono::{DateTime, Utc};
use ft_core::{Record, TrustState};

use crate::engine::IndexDoc;
use crate::kind::{DocId, IndexKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_doc_has_id_and_owners_in_body() {
        let scope = ft_scope::Scope {
            id: "apps/checkout".into(),
            name: "Checkout".into(),
            applies_to_patterns: vec!["apps/checkout/**".into()],
            applies_to: vec![],
            aliases: vec!["checkout".into()],
            codeowners: None,
        };
        let doc = scope_doc(&scope, Utc::now());
        assert_eq!(doc.kind, IndexKind::Scope);
        assert_eq!(doc.id, DocId::Synthetic { kind: IndexKind::Scope, key: "apps/checkout".into() });
        assert_eq!(doc.trust, TrustState::Verified);
        assert!(doc.body.contains("apps/checkout/**"));
        assert!(doc.body.contains("checkout"));
    }

    #[test]
    fn identity_doc_indexes_emails_and_caps() {
        let ident = ft_identity::RegisteredIdentity {
            id: "alice".into(),
            name: "Alice".into(),
            kind: ft_identity::IdentityKind::Human,
            emails: vec!["alice@example.com".into()],
            machines: vec![],
            capabilities: Default::default(),
            status: Default::default(),
        };
        let doc = identity_doc(&ident, Utc::now());
        assert_eq!(doc.kind, IndexKind::Identity);
        assert_eq!(doc.id, DocId::Synthetic { kind: IndexKind::Identity, key: "alice".into() });
        assert_eq!(doc.trust, TrustState::Verified);
        assert!(doc.body.contains("alice@example.com"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ft-search --lib sources::`
Expected: FAIL — `scope_doc`/`identity_doc` not defined (and ft-scope/ft-identity not yet deps).

- [ ] **Step 3: Add deps + implement builders**

In `crates/ft-search/Cargo.toml`, under `[dependencies]`, add (use workspace versions, matching how other crates reference them):

```toml
ft-scope = { path = "../ft-scope" }
ft-identity = { path = "../ft-identity" }
```

> Confirm the exact dependency style by copying the pattern another crate (e.g. ft-cli) uses for `ft-scope` / `ft-identity` — path vs workspace. Match it.

Implement in `crates/ft-search/src/sources.rs` (above the test module):

```rust
/// Lower a scope into a searchable document. CODEOWNERS owner logins (the
/// "rules" the epic referred to) are folded into the body.
#[must_use]
pub fn scope_doc(scope: &ft_scope::Scope, updated_at: DateTime<Utc>) -> IndexDoc {
    let mut body_parts: Vec<String> = Vec::new();
    body_parts.push(scope.id.clone());
    if !scope.aliases.is_empty() {
        body_parts.push(scope.aliases.join(" "));
    }
    body_parts.extend(scope.applies_to_patterns.iter().cloned());
    if let Some(entries) = &scope.codeowners {
        for entry in entries {
            body_parts.push(entry.owners.join(" "));
        }
    }
    IndexDoc {
        id: DocId::Synthetic { kind: IndexKind::Scope, key: scope.id.clone() },
        kind: IndexKind::Scope,
        title: scope.name.clone(),
        body: body_parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"),
        trust: TrustState::Verified,
        owning_scope: Some(scope.id.clone()),
        updated_at,
    }
}

/// Lower a registered identity into a searchable document.
#[must_use]
pub fn identity_doc(ident: &ft_identity::RegisteredIdentity, updated_at: DateTime<Utc>) -> IndexDoc {
    let mut body_parts: Vec<String> = Vec::new();
    body_parts.push(ident.id.clone());
    body_parts.extend(ident.emails.iter().cloned());
    body_parts.extend(ident.machines.iter().cloned());
    let caps = ident.effective_capabilities();
    for (name, enabled) in caps.extra.iter() {
        if *enabled {
            body_parts.push(name.clone());
        }
    }
    IndexDoc {
        id: DocId::Synthetic { kind: IndexKind::Identity, key: ident.id.clone() },
        kind: IndexKind::Identity,
        title: if ident.name.is_empty() { ident.id.clone() } else { ident.name.clone() },
        body: body_parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"),
        trust: TrustState::Verified,
        owning_scope: None,
        updated_at,
    }
}

/// Lower every history entry of a record into per-entry audit documents.
/// Each entry's trust inherits the record's indexed trust state.
#[must_use]
pub fn audit_docs(record: &Record, record_trust: TrustState) -> Vec<IndexDoc> {
    let rec_id = record.envelope.id.as_str();
    let rec_title = record.envelope.title.as_str();
    record
        .history
        .iter()
        .enumerate()
        .map(|(n, entry)| {
            let op = entry.ops_summary.first().cloned().unwrap_or_else(|| "history".into());
            let mut body_parts: Vec<String> = Vec::new();
            body_parts.push(entry.primary_actor.as_str().to_string());
            for c in &entry.contributors {
                body_parts.push(c.as_str().to_string());
            }
            body_parts.extend(entry.ops_summary.iter().cloned());
            body_parts.push(format!("{} -> {}", entry.from_hash, entry.to_hash));
            IndexDoc {
                id: DocId::Synthetic {
                    kind: IndexKind::Audit,
                    key: format!("{rec_id}#h{n}"),
                },
                kind: IndexKind::Audit,
                title: format!("{op}: {rec_title}"),
                body: body_parts.join("\n"),
                trust: record_trust,
                owning_scope: record.envelope.owning_scope.clone(),
                updated_at: entry.timestamp,
            }
        })
        .collect()
}
```

> `IndexDoc` is defined in `engine.rs`; make it `pub` and ensure `crate::engine::IndexDoc` is importable (it is `pub struct` already per Task 3). Add `pub use engine::IndexDoc;` to `lib.rs` if you want it in the public API for the reindex command (you do — Task 6 uses it).

In `crates/ft-search/src/lib.rs` add:

```rust
mod sources;
```
```rust
pub use engine::{IndexDoc, SearchEngine};
pub use sources::{audit_docs, identity_doc, scope_doc};
```

(Replace the existing `pub use engine::SearchEngine;` line.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ft-search --lib sources::`
Expected: PASS (2 tests).

> If `caps.extra` field access or `effective_capabilities()` shape differs, simplify the identity body to id+emails+machines and drop capability text — do not block on capability internals.

- [ ] **Step 5: Commit**

```bash
git add crates/ft-search/src/sources.rs crates/ft-search/src/lib.rs crates/ft-search/Cargo.toml Cargo.lock
git commit -m "feat(ft-search): scope/identity/audit IndexDoc builders"
```

---

## Task 6: Reindex wiring + best-effort embedding

**Files:**
- Modify: `crates/ft-cli/src/commands/index_cmd.rs`

- [ ] **Step 1: Add a shared synthetic-indexing helper**

In `crates/ft-cli/src/commands/index_cmd.rs`, add imports and a helper that both `rebuild` and `refresh` call after their record loop:

```rust
use chrono::Utc;
use ft_scope::ScopeRegistry;
use ft_search::IndexDoc;
```

```rust
/// Index scopes, identities, and per-entry audit history as synthetic
/// documents. `records` is the set of records already read in this pass (audit
/// entries are extracted from them). Returns the number of synthetic docs
/// upserted. Embedding is dispatched best-effort; failures degrade to
/// lexical-only and are surfaced as warnings.
fn index_synthetic_docs(
    cmd: &'static str,
    ws: &crate::workspace::Workspace,
    engine: &ft_search::SearchEngine,
    records: &[ft_core::Record],
    warnings: &mut Vec<String>,
) -> Result<usize, CliError> {
    let now = Utc::now();
    let mut docs: Vec<IndexDoc> = Vec::new();

    // Scopes (CODEOWNERS rules ride along in the body).
    match ScopeRegistry::load(&ws.root) {
        Ok(reg) => {
            for scope in reg.scopes() {
                docs.push(ft_search::scope_doc(scope, now));
            }
        }
        Err(e) => warnings.push(format!("scope index skipped: {e}")),
    }

    // Identities.
    match ft_identity::load_registry(&ws.root) {
        Ok(reg) => {
            for ident in &reg.identities {
                docs.push(ft_search::identity_doc(ident, now));
            }
        }
        Err(e) => warnings.push(format!("identity index skipped: {e}")),
    }

    // Audit entries: per history entry of each record. Trust inherits the
    // record's indexed trust (same default the engine would assign).
    for rec in records {
        let trust = ft_search_record_trust(rec);
        docs.extend(ft_search::audit_docs(rec, trust));
    }

    // Lexical upsert for every synthetic doc.
    for doc in &docs {
        engine
            .upsert_document(doc)
            .map_err(|e| CliError::internal(cmd, format!("upsert synthetic doc: {e}")))?;
    }

    // Best-effort embedding via the daemon (mock-fallback safe).
    dispatch_synthetic_embeddings(ws, &docs, warnings);

    Ok(docs.len())
}

/// Trust an audit doc should inherit — mirror the engine's record-trust rule
/// (memory bodies carry trust; work kinds default to reviewed). We reuse the
/// embed text path's record by reading the body's trust if present, else a
/// kind default. Kept local to avoid leaking engine internals.
fn ft_search_record_trust(rec: &ft_core::Record) -> ft_core::TrustState {
    use ft_core::{RecordBody, RecordKind, TrustState};
    match &rec.body {
        RecordBody::Incident(b) => b.trust,
        RecordBody::Finding(b) => b.trust,
        RecordBody::Runbook(b) => b.trust,
        RecordBody::Decision(b) => b.trust,
        RecordBody::Gotcha(b) => b.trust,
        RecordBody::Memory(b) => b.trust,
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            match rec.envelope.kind {
                RecordKind::Epic | RecordKind::Task | RecordKind::Subtask | RecordKind::Bug => {
                    TrustState::Reviewed
                }
                _ => TrustState::Draft,
            }
        }
    }
}

/// Send IndexRecord requests for synthetic docs so their vectors land. Best
/// effort: a missing/failed daemon leaves the docs lexical-only.
fn dispatch_synthetic_embeddings(
    ws: &crate::workspace::Workspace,
    docs: &[IndexDoc],
    warnings: &mut Vec<String>,
) {
    if docs.is_empty() {
        return;
    }
    let socket = match ws.daemon_socket_path() {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("synthetic-doc embedding skipped: {e}"));
            return;
        }
    };
    if let Err(e) = crate::commands::daemon_cmd::ensure_running("index", ws) {
        warnings.push(format!("synthetic-doc embedding skipped (no daemon): {e}"));
        return;
    }
    for doc in docs {
        let text = doc.embed_text();
        if let Err(e) =
            ft_embed::daemon::send_index_record(&socket, &doc.id.as_storage_str(), &text)
        {
            warnings.push(format!("embed {} failed: {e}", doc.id.as_storage_str()));
            // Keep going; lexical row already exists.
        }
    }
}
```

> Verify helper names against the codebase while implementing: `ws.daemon_socket_path()` and `daemon_cmd::ensure_running(cmd, ws)` are used in `search.rs` exactly this way; `ScopeRegistry::load`, `reg.scopes()`, `ft_identity::load_registry`, `reg.identities` are confirmed APIs. `ws.root` is the workspace root `Path` (confirm the field name on `Workspace`; it may be `ws.root` or a method).

- [ ] **Step 2: Call the helper from `rebuild` and `refresh`**

In `rebuild`, collect records into a `Vec` as you upsert them, then call the helper. Replace the record loop + outcome construction:

```rust
    let mut search_rows = 0usize;
    let mut warnings: Vec<String> = Vec::new();
    let mut records: Vec<ft_core::Record> = Vec::new();
    for row in storage.iter(&StorageFilter::default()) {
        let rec = row.map_err(|e| CliError::internal(CMD_REBUILD, format!("read record: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(CMD_REBUILD, format!("upsert search: {e}")))?;
        search_rows += 1;
        records.push(rec);
    }
    let synthetic = index_synthetic_docs(CMD_REBUILD, &ws, &engine, &records, &mut warnings)?;
    search_rows += synthetic;

    Ok(CommandOutcome::IndexAction(IndexActionOutcome {
        command: CMD_REBUILD,
        action: "rebuild",
        records_indexed: report.records_indexed,
        records_changed: report.records_indexed,
        search_rows_upserted: search_rows,
        warnings,
    }))
```

Apply the analogous change in `refresh` (it already reads each `rec` in a loop — push into a `records` vec, then call `index_synthetic_docs(CMD_REFRESH, &ws, &engine, &records, &mut warnings)` and add the count + warnings to the outcome).

- [ ] **Step 3: Build**

Run: `cargo build -p ft-cli`
Expected: clean build. Fix any field/method-name mismatches the compiler flags (see the verify note in Step 1).

- [ ] **Step 4: Commit**

```bash
git add crates/ft-cli/src/commands/index_cmd.rs
git commit -m "feat(ft-cli): index scopes/identities/audit at rebuild+refresh with best-effort embedding"
```

---

## Task 7: CLI `--kind scope|identity|audit` + hit view

**Files:**
- Modify: `crates/ft-cli/src/cli.rs`
- Modify: `crates/ft-cli/src/commands/search.rs`

- [ ] **Step 1: Add `SearchKindArg`**

In `crates/ft-cli/src/cli.rs`, add a search-specific kind enum (leave `AnyKindArg` and `prime` untouched):

```rust
/// Kind selector for `firetrail search --kind`. Superset of record kinds plus
/// the synthetic search domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum SearchKindArg {
    /// Epic. Task. Subtask. Bug. Incident. Finding. Runbook. Decision. Gotcha. Memory.
    Epic, Task, Subtask, Bug, Incident, Finding, Runbook, Decision, Gotcha, Memory,
    /// Scope definition.
    Scope,
    /// Registered identity.
    Identity,
    /// Audit/history entry.
    Audit,
}

impl SearchKindArg {
    /// Convert to `ft_search::IndexKind`.
    #[must_use]
    pub fn to_index_kind(self) -> ft_search::IndexKind {
        use ft_core::RecordKind as R;
        use ft_search::IndexKind as I;
        match self {
            Self::Epic => I::Record(R::Epic),
            Self::Task => I::Record(R::Task),
            Self::Subtask => I::Record(R::Subtask),
            Self::Bug => I::Record(R::Bug),
            Self::Incident => I::Record(R::Incident),
            Self::Finding => I::Record(R::Finding),
            Self::Runbook => I::Record(R::Runbook),
            Self::Decision => I::Record(R::Decision),
            Self::Gotcha => I::Record(R::Gotcha),
            Self::Memory => I::Record(R::Memory),
            Self::Scope => I::Scope,
            Self::Identity => I::Identity,
            Self::Audit => I::Audit,
        }
    }
}
```

> Doc-comment each variant individually if `#![deny(missing_docs)]` / clippy requires it; the combined comment above is shorthand for the plan.

Change `SearchArgs.kinds`:

```rust
    /// Restrict to a kind (record kinds + scope/identity/audit). Repeatable.
    #[arg(long = "kind", value_enum)]
    pub kinds: Vec<SearchKindArg>,
```

- [ ] **Step 2: Update `search.rs` mapping + hit view + quarantine**

In `crates/ft-cli/src/commands/search.rs`, change the kind-filter mapping:

```rust
    if !args.kinds.is_empty() {
        query.kind_filter = args.kinds.iter().map(|k| k.to_index_kind()).collect();
    }
```

Fix the quarantine filter to skip synthetic docs (they have no storage record):

```rust
        .filter_map(|h| {
            let quarantined = match h.id.as_record_id() {
                Some(rid) => ctx.storage.read(rid).map(|rec| is_quarantined(&rec)).unwrap_or(false),
                None => false, // synthetic docs are never quarantined
            };
            if quarantined && !args.include_quarantine {
                return None;
            }
            let mut view = SearchHitView::from(h);
            if quarantined {
                view.quarantine = true;
            }
            Some(view)
        })
```

Fix `From<SearchHit> for SearchHitView` (id + kind now `DocId`/`IndexKind`):

```rust
impl From<SearchHit> for SearchHitView {
    fn from(h: SearchHit) -> Self {
        Self {
            id: h.id.as_storage_str(),
            kind: h.kind.label().to_string(),
            title: h.title,
            score: h.score,
            trust: serde_lower(&h.trust),
            mode: hit_mode_label(h.mode),
            quarantine: false,
        }
    }
}
```

The `similar` path also builds `SearchHitView::from`; it compiles unchanged once `From` is fixed. If `similar` calls `ctx.resolve_id` / `storage.read(&h.id)` anywhere, apply the same `as_record_id()` guard.

- [ ] **Step 3: Build + existing CLI search tests**

Run: `cargo build -p ft-cli && cargo test -p ft-cli search`
Expected: clean build; existing search tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/ft-cli/src/cli.rs crates/ft-cli/src/commands/search.rs
git commit -m "feat(ft-cli): firetrail search --kind scope|identity|audit + DocId hit view"
```

---

## Task 8: Update ft-ops + ft-ui consumers

**Files:**
- Modify: `crates/ft-ops/src/memory/search.rs`
- Modify: `crates/ft-ops/src/memory/create.rs` (where `MemoryKind` is defined — add `to_index_kind`)
- Modify: `crates/ft-ui/src/routes/memory.rs` (only if it breaks)

- [ ] **Step 1: Add `MemoryKind::to_index_kind`**

In `crates/ft-ops/src/memory/create.rs` (the `MemoryKind` enum's impl block), add alongside the existing `to_core`:

```rust
    /// Convert to the search-layer kind.
    #[must_use]
    pub fn to_index_kind(self) -> ft_search::IndexKind {
        ft_search::IndexKind::Record(self.to_core())
    }
```

> If `to_core()` returns `RecordKind`, this wrapper is exact. Confirm `ft_search` is a dependency of `ft-ops` (it is — `search.rs` imports it).

- [ ] **Step 2: Fix the ops search mapping, hit view, quarantine**

In `crates/ft-ops/src/memory/search.rs`:

```rust
    if !input.kinds.is_empty() {
        query.kind_filter = input.kinds.iter().map(|k| k.to_index_kind()).collect();
    }
```

```rust
impl From<SearchHit> for SearchHitOut {
    fn from(h: SearchHit) -> Self {
        Self {
            id: h.id.as_storage_str(),
            kind: h.kind.label().to_string(),
            title: h.title,
            score: h.score,
            trust: serialize_lower(&h.trust),
            mode: hit_mode_label(h.mode).to_string(),
            quarantine: false,
        }
    }
}
```

Quarantine filter:

```rust
        .filter_map(|h| {
            let quarantined = match h.id.as_record_id() {
                Some(rid) => ctx.storage.read(rid).map(|rec| is_quarantined(&rec)).unwrap_or(false),
                None => false,
            };
            if quarantined && !input.include_quarantine {
                return None;
            }
            let mut view = SearchHitOut::from(h);
            if quarantined {
                view.quarantine = true;
            }
            Some(view)
        })
```

- [ ] **Step 3: Build the workspace; fix ft-ui only if it breaks**

Run: `cargo build --workspace`
Expected: clean build. If `ft-ui/src/routes/memory.rs` references `hit.id`/`hit.kind` directly, apply the same `as_storage_str()` / `label()` substitution. The UI consumes `SearchHitOut`/`SearchHitView` (string fields), so it most likely needs no change.

- [ ] **Step 4: Commit**

```bash
git add crates/ft-ops/src/memory/search.rs crates/ft-ops/src/memory/create.rs
git commit -m "feat(ft-ops): map search kinds to IndexKind; DocId-aware hit view + quarantine"
```

---

## Task 9: End-to-end acceptance test

**Files:**
- Create/modify: `crates/ft-cli/tests/m3_search_prime_daemon.rs` (add a test)

- [ ] **Step 1: Write the failing acceptance test**

Add a test that builds a workspace with a scope + identity + a record with history, runs `index rebuild`, then asserts `firetrail search --kind scope|identity|audit` returns hits. Follow the existing test harness in this file (it already exercises `index rebuild` + `search`). Sketch (adapt to the file's existing helpers for spawning the CLI / building a workspace):

```rust
#[test]
fn search_finds_scopes_identities_and_audit_after_rebuild() {
    let ws = TestWorkspace::init();               // existing helper
    ws.write_scopes_yaml(r#"
scopes:
  - id: apps/checkout
    name: Checkout
    applies_to: ["apps/checkout/**"]
    aliases: ["checkout"]
"#);
    ws.write_identities_yaml(r#"
identities:
  - id: alice
    name: Alice
    kind: human
    emails: ["alice@example.com"]
"#);
    // A record with at least one history entry (created via a normal CLI
    // create + an update, or a fixture writer the harness already provides).
    let _task = ws.create_task("Fix checkout latency");

    ws.run(&["index", "rebuild"]).assert_success();

    let scope_hits = ws.run_json(&["search", "checkout", "--kind", "scope"]);
    assert!(scope_hits.hits.iter().any(|h| h.kind == "scope" && h.id == "scope:apps/checkout"));

    let id_hits = ws.run_json(&["search", "alice", "--kind", "identity"]);
    assert!(id_hits.hits.iter().any(|h| h.kind == "identity" && h.id == "identity:alice"));

    let audit_hits = ws.run_json(&["search", "create", "--kind", "audit"]);
    assert!(audit_hits.hits.iter().any(|h| h.kind == "audit"));
}
```

> Use the file's real helpers (`TestWorkspace`, `run`, `run_json`, fixture writers). If there is no `write_scopes_yaml`/`write_identities_yaml` helper, write the YAML files directly under `<ws>/.firetrail/` with `std::fs::write`. If creating a record with history via the CLI is awkward, assert scope+identity hits (the core acceptance) and cover audit via a focused `ft-search` unit test using `audit_docs` + `upsert_document` + `search`.

- [ ] **Step 2: Run to verify it fails (then passes)**

Run: `cargo test -p ft-cli --test m3_search_prime_daemon search_finds_scopes_identities_and_audit_after_rebuild`
Expected: FAIL first (if written before earlier tasks land) → PASS after the implementation tasks.

- [ ] **Step 3: Full gates**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all green. Fix clippy/fmt findings inline.

- [ ] **Step 4: Commit**

```bash
git add crates/ft-cli/tests/m3_search_prime_daemon.rs
git commit -m "test(ft-cli): e2e search across scope/identity/audit after rebuild"
```

---

## Task 10: Close out

- [ ] **Step 1: Update the issue + verify gates**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 2: Mark acceptance in the spec** — confirm all three acceptance criteria from the spec are met:
  - `firetrail search 'X' --kind scope|identity|audit` returns matching records ✔ (Task 9)
  - `index rebuild` backfills the three domains ✔ (Task 6)
  - tests cover ≥1 hit per domain ✔ (Tasks 5, 9)

- [ ] **Step 3: Push + close** (per CLAUDE.md session protocol)

```bash
git lfs push --all origin firetrail-8z0m.3-index-missing-domains   # LFS objects (model) — manual here
git push -u origin firetrail-8z0m.3-index-missing-domains
bd close firetrail-8z0m.3 --reason "scopes/identities/audit indexed (lexical+vector) and searchable via --kind"
bd dolt push
```

---

## Notes for the implementing engineer

- **Verify field/method names before inventing them.** The plan flags every spot where a field path (`record.envelope.owning_scope`, `ws.root`, `caps.extra`) should be confirmed against the source. If a path differs, adapt — never fabricate a field.
- **TDD discipline:** each task writes the test first, watches it fail, implements, watches it pass.
- **The big task (Task 3) is unavoidably cross-cutting** — the type change touches several call sites at once. Lean on the compiler: change the public types, then fix every error it reports. The listed substitutions are the complete set.
- **Vector assertions are feature-gated.** `sqlite-vec` is default-on; if running `--no-default-features`, the embedding dispatch and `records_vec` are absent and synthetic docs are lexical-only. Acceptance tests assert lexical hits (always available) and treat vectors as a bonus.
- **`git lfs push` is manual** in this repo (`core.hooksPath=.beads/hooks` bypasses the lfs pre-push hook) — see the session memory.
