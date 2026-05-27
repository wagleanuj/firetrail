# ft-index — SQLite read index and queries (M1 form)

**Epic:** `firetrail-oa3`
**Wave:** 2
**Depends on:** ft-core, ft-storage, ft-testkit
**Depended on by:** ft-cli, ft-search (M3), ft-pr (M4)

---

## Purpose

`ft-index` maintains a SQLite database of indexed record metadata, derived from
the JSON record files. It exists because walking thousands of JSON files for
every list, ready-detection, or dependency-walk query is slow.

The index is **derived data**: rebuildable from `ft-storage` at any time. It is
gitignored. Treat queries against it as cache reads; queries that must reflect
the absolute current state of disk go via `ft-storage` directly.

At M1, only the metadata index is implemented. Vector tables (`sqlite-vec`) are
added in M3 alongside `ft-search`.

---

## Public API

### Index

```rust
pub struct Index {
    db_path: PathBuf,
    conn: rusqlite::Connection,
}

impl Index {
    /// Open or create the index database at .firetrail/index.db.
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, IndexError>;

    /// Schema version applied to this database.
    pub fn schema_version(&self) -> u32;

    /// Apply pending migrations.
    pub fn migrate(&mut self) -> Result<(), IndexError>;

    /// Rebuild the entire index from a Storage implementation.
    /// Used on first command after corruption or schema bump.
    pub fn rebuild_from(&mut self, storage: &dyn Storage) -> Result<RebuildReport, IndexError>;

    /// Diff-driven refresh: re-index the listed records, removing deleted ones.
    /// Hook-driven: post-checkout and post-merge hooks call this with the set of
    /// changed paths.
    pub fn refresh(&mut self, storage: &dyn Storage,
                   changed: &[PathBuf], removed: &[PathBuf])
        -> Result<RefreshReport, IndexError>;

    // ── Reads ────────────────────────────────────────────────────────────────
    pub fn show(&self, id: &RecordId) -> Result<IndexedRecord, IndexError>;
    pub fn list(&self, query: &ListQuery) -> Result<Vec<IndexedRecord>, IndexError>;
    pub fn ready(&self, query: &ReadyQuery) -> Result<Vec<IndexedRecord>, IndexError>;
    pub fn dependency_walk(&self, root: &RecordId, direction: WalkDirection, max_depth: usize)
        -> Result<Vec<DepEdge>, IndexError>;
    pub fn relations(&self, id: &RecordId) -> Result<Vec<DepEdge>, IndexError>;
    pub fn child_records(&self, parent: &RecordId) -> Result<Vec<RecordId>, IndexError>;
    pub fn count(&self, query: &ListQuery) -> Result<u64, IndexError>;
}
```

### Query types

```rust
#[derive(Default, Clone, Debug)]
pub struct ListQuery {
    pub kinds: Option<Vec<RecordKind>>,
    pub statuses: Option<Vec<Status>>,
    pub owners: Option<Vec<Identity>>,
    pub scopes: Option<Vec<String>>,
    pub labels: Vec<(String, String)>,
    pub parent: Option<RecordId>,           // child-of relation
    pub created_since: Option<DateTime<Utc>>,
    pub updated_since: Option<DateTime<Utc>>,
    pub include_closed: bool,               // default false
    pub include_archived: bool,             // default false
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub order_by: OrderBy,
}

#[derive(Default, Clone, Debug)]
pub struct ReadyQuery {
    pub kinds: Option<Vec<RecordKind>>,
    pub owners: Option<Vec<Identity>>,
    pub scopes: Option<Vec<String>>,
    pub include_claimed: bool,              // default false
    pub limit: Option<u64>,
}

#[derive(Default, Clone, Debug)]
pub enum OrderBy {
    #[default]
    Priority,                               // P0 first
    CreatedAt,
    UpdatedAt,
    Title,
}

pub enum WalkDirection {
    Upstream,                               // follow blocked-by
    Downstream,                             // follow blocks
    Both,
}
```

### Result types

```rust
pub struct IndexedRecord {
    pub id: RecordId,
    pub kind: RecordKind,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub owner: Option<Identity>,
    pub created_by: Identity,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub owning_scope: Option<String>,
    pub claim: Option<Claim>,
    pub blocked_by_count: u32,
    pub blocks_count: u32,
    pub parent_id: Option<RecordId>,
}

pub struct DepEdge {
    pub from: RecordId,
    pub to: RecordId,
    pub kind: RelationKind,
    pub depth: u32,                         // distance from walk root
}

pub struct RebuildReport {
    pub records_indexed: u64,
    pub relations_indexed: u64,
    pub elapsed: Duration,
}

pub struct RefreshReport {
    pub records_added: u64,
    pub records_updated: u64,
    pub records_removed: u64,
    pub elapsed: Duration,
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("database: {0}")] Db(#[from] rusqlite::Error),
    #[error("schema migration failed: {0}")] Migration(String),
    #[error("storage: {0}")] Storage(#[from] StorageError),
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("integrity check failed: {0}")] Integrity(String),
}
```

---

## Schema (M1)

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE schema_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Rows include 'schema_version', 'last_indexed_commit', 'last_rebuild_at'.

CREATE TABLE records (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    priority TEXT NOT NULL,
    owner TEXT,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,        -- ISO-8601
    updated_at TEXT NOT NULL,
    closed_at TEXT,
    owning_scope TEXT,
    state_hash TEXT NOT NULL,
    file_path TEXT NOT NULL,
    file_mtime INTEGER NOT NULL,
    origin TEXT NOT NULL
);
CREATE INDEX records_kind_status ON records(kind, status);
CREATE INDEX records_owner ON records(owner);
CREATE INDEX records_scope ON records(owning_scope);
CREATE INDEX records_updated_at ON records(updated_at);

CREATE TABLE labels (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (record_id, key, value)
);
CREATE INDEX labels_key_value ON labels(key, value);

CREATE TABLE affected_scopes (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    PRIMARY KEY (record_id, scope)
);

CREATE TABLE applies_to (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    glob TEXT NOT NULL,
    PRIMARY KEY (record_id, glob)
);

CREATE TABLE relations (
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL,
    created_by TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id, kind)
);
CREATE INDEX relations_to ON relations(to_id, kind);
CREATE INDEX relations_from ON relations(from_id, kind);

CREATE TABLE claims (
    record_id TEXT PRIMARY KEY REFERENCES records(id) ON DELETE CASCADE,
    claimed_by TEXT NOT NULL,
    claimed_at TEXT NOT NULL,
    claim_source TEXT NOT NULL,
    claim_expires_at TEXT NOT NULL
);

CREATE TABLE acceptance_criteria (
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

CREATE TABLE evidence (
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
```

### Schema migrations

A migrations module owns ordered `up`/`down` scripts keyed by version number.
`Index::open` applies pending migrations automatically; the version is recorded
in `schema_meta`. Forward-incompatible schema changes refuse to open and require
a manual `firetrail index rebuild`.

---

## Internal design

### Rebuild flow

```
1. Drop all data tables (keep schema_meta).
2. Apply current schema.
3. Iterate storage.iter(StorageFilter::default()).
4. For each record:
   - Insert into records.
   - Insert labels, affected_scopes, applies_to rows.
   - Insert claim row if present.
   - Insert acceptance_criteria, evidence rows.
5. Walk all records again, inserting relations rows from the relation graph
   (currently encoded inline in records via fields; M2+ adds an explicit
   relations file under .firetrail/relations/ if needed).
6. Update schema_meta last_rebuild_at and last_indexed_commit (from ft-git head()).
7. Commit transaction.
```

The rebuild runs in a single SQLite transaction. Either fully succeeds or leaves
the previous index intact (write-ahead log rollback).

### Refresh flow

Hook-driven: `post-checkout` and `post-merge` pass changed and removed paths via
arguments. The refresh:

1. For each removed path, delete the corresponding record + cascaded rows.
2. For each changed path, re-read via `ft-storage`, upsert all rows.
3. Update `last_indexed_commit`.

If too many paths changed (configurable threshold, default 500), refresh falls
back to a full rebuild.

### Ready detection

```sql
SELECT r.* FROM records r
WHERE r.status NOT IN ('closed', 'deferred', 'archived')
  AND NOT EXISTS (
    SELECT 1 FROM relations rel
    INNER JOIN records blocker ON blocker.id = rel.to_id
    WHERE rel.from_id = r.id
      AND rel.kind = 'blocked-by'
      AND blocker.status NOT IN ('closed', 'deferred', 'archived')
  )
  AND (
    ?include_claimed = 1 OR
    NOT EXISTS (
      SELECT 1 FROM claims c
      WHERE c.record_id = r.id
        AND c.claim_expires_at > ?now
    )
  )
  -- plus scope, owner, kind filters from ReadyQuery
ORDER BY priority ASC, updated_at DESC
LIMIT ?limit;
```

### Dependency walk

Recursive CTE in SQLite. Bounded by `max_depth` to avoid runaway on cyclic
graphs (cycles exist if force-pushed badly; the walk detects and reports).

---

## Performance targets (M1)

| Query | Records | Target |
|---|---|---|
| `list` with simple filter | 1,000 | < 100 ms |
| `ready` | 1,000 | < 200 ms |
| `dependency_walk` to depth 5 | 100 | < 50 ms |
| Full rebuild | 1,000 | < 5 s |
| Incremental refresh | 10 changed | < 200 ms |

Performance tests use `ft-testkit` to generate fixture corpora.

---

## Acceptance

1. `Index::open` on a fresh workspace creates the schema and opens cleanly.
2. `rebuild_from` produces query results identical to direct `ft-storage::iter`
   reads (no records lost, no extras introduced).
3. `refresh` correctly handles add, modify, and delete deltas — both via the
   explicit API and via the post-checkout hook path.
4. `ready` excludes records with open blockers; includes them after the blocker
   is closed.
5. `list` honors every filter dimension in `ListQuery`.
6. `dependency_walk` handles cycles without infinite recursion and reports the
   cycle in `DepEdge` output.
7. Schema-version mismatch (open an index from a future version) refuses to
   open, instructs the user to upgrade or rebuild.
8. Performance gates above are met against a 1,000-record fixture.
9. WAL mode is enabled; concurrent readers do not block on a writer.

---

## Testing requirements

- Unit tests for each query type independently.
- Property tests for filter composition (round-trip a ListQuery through SQL
  construction and verify expected result counts against a fixture).
- Integration test: build a fixture corpus via `ft-testkit`, write via
  `ft-storage`, rebuild index, run queries, assert.
- Performance test (gated behind `--features slow-tests`) for the targets above.
- Doc tests on every public method.

---

## Out of scope (deferred)

- Vector tables (`sqlite-vec` + embedding columns) — M3 in `ft-search`.
- Full-text search beyond `LIKE` — M3.
- Scope-distance ranking — M3 (ft-search) and M5 (multi-scope semantics).
- History queries — M2 (ft-history).
- Quarantine index — M6 (ft-import).

---

## References

- ADR-0002 — JSON-in-Git storage (index is derived)
- ADR-0007 — Local embeddings (same SQLite database holds vector tables in M3)
- ADR-0015 — Hash-based IDs
- ADR-0016 — Build approach
