# ft-storage — JSON-in-Git storage layer (embedded mode)

**Epic:** `firetrail-qrv`
**Wave:** 2
**Depends on:** ft-core, ft-git, ft-testkit
**Depended on by:** ft-cli, ft-index, ft-history (M2), ft-pr (M4), ft-import (M6)

---

## Purpose

`ft-storage` is the canonical read/write layer for record files. Records are
serialized to JSON files under `.firetrail/records/<type>/<id>.json`. Writes are
atomic. Reads can target any git ref.

At M1, only embedded storage mode is implemented. External mode (cloning a
separate data repo) is added in M5 (ADR-0006); the `Storage` trait is designed
so that addition is non-breaking.

---

## Public API

### The Storage trait

```rust
pub trait Storage: Send + Sync {
    /// Read a record by ID from the working tree.
    fn read(&self, id: &RecordId) -> Result<Record, StorageError>;

    /// Read a record at a specific git ref. Used by history walks and check pr.
    fn read_at_ref(&self, gitref: &str, id: &RecordId) -> Result<Record, StorageError>;

    /// Write a record. Atomic: writes to .tmp then renames.
    /// Returns the resolved file path.
    fn write(&self, record: &Record) -> Result<PathBuf, StorageError>;

    /// Delete a record file from the working tree. Does not commit.
    fn delete(&self, id: &RecordId) -> Result<(), StorageError>;

    /// List records, optionally filtered. Returns IDs only — fetch bodies via read().
    fn list(&self, filter: &StorageFilter) -> Result<Vec<RecordId>, StorageError>;

    /// Stream records matching the filter. Useful for index rebuild.
    fn iter<'a>(&'a self, filter: &'a StorageFilter)
        -> Box<dyn Iterator<Item = Result<Record, StorageError>> + 'a>;

    /// Path on disk where a record's file lives.
    fn path_for(&self, id: &RecordId) -> PathBuf;

    /// Root of the records tree (e.g. .firetrail/records/).
    fn records_root(&self) -> PathBuf;
}
```

### EmbeddedStorage

```rust
pub struct EmbeddedStorage {
    repo_root: PathBuf,
    git: Arc<Repo>,
}

impl EmbeddedStorage {
    /// Open storage rooted at the given Firetrail workspace.
    /// Expects .firetrail/records/ to exist or creates it.
    pub fn open(repo_root: impl Into<PathBuf>) -> Result<Self, StorageError>;

    /// Initialize an empty records tree at .firetrail/records/.
    pub fn init(repo_root: impl Into<PathBuf>) -> Result<Self, StorageError>;
}

impl Storage for EmbeddedStorage { /* ... */ }
```

### StorageFilter

```rust
#[derive(Default, Clone, Debug)]
pub struct StorageFilter {
    pub kinds: Option<Vec<RecordKind>>,
    pub statuses: Option<Vec<Status>>,
    pub owners: Option<Vec<Identity>>,
    pub scopes: Option<Vec<String>>,
    pub labels: Vec<(String, String)>,
    pub modified_since: Option<DateTime<Utc>>,
}

impl StorageFilter {
    pub fn kind(mut self, k: RecordKind) -> Self;
    pub fn status(mut self, s: Status) -> Self;
    pub fn owner(mut self, o: Identity) -> Self;
    pub fn scope(mut self, s: impl Into<String>) -> Self;
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self;
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("record not found: {0}")]
    NotFound(RecordId),

    #[error("invalid record on disk at {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },

    #[error("hash mismatch for {id}: file says {file_hash}, recompute says {recomputed}")]
    HashMismatch { id: RecordId, file_hash: String, recomputed: String },

    #[error("workspace not initialized: {0}")]
    NotInitialized(PathBuf),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("git: {0}")]
    Git(#[from] GitError),

    #[error("core: {0}")]
    Core(#[from] CoreError),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}
```

---

## File layout

```
<repo_root>/.firetrail/
├── config.yml                      (managed by ft-cli)
├── identity.yml                    (M1: optional; M5: required for registry)
├── index.db                        (managed by ft-index; gitignored)
├── records/
│   ├── task/
│   │   ├── task-7f2a915c.....json
│   │   ├── task-9c4b2e7a.....json
│   │   └── ...
│   ├── epic/
│   ├── subtask/
│   ├── bug/
│   ├── incident/   (declared in M1; populated in M2+)
│   ├── finding/
│   ├── runbook/
│   ├── decision/
│   ├── gotcha/
│   └── memory/
└── exports/                        (optional Markdown exports; later milestones)
```

### Filename rules

- Lowercase IDs always (ADR-0015).
- Filename is `<lowercase_id>.json`. The `id` field inside the file uses the same
  lowercase form.
- `<type>` directory name is the lowercase form of the record kind (e.g. `task`,
  `subtask`, `gotcha`).
- No nesting beyond `<type>/<id>.json`. Records do not have sub-directories.

---

## Internal design

### Atomic write

```
1. Compute the canonical JSON.
2. Verify record.envelope.state_hash matches sha256(canonical_json_without_hashes).
   If mismatch, return StorageError::HashMismatch (caller is responsible for
   updating state_hash before passing to write()).
3. Create parent directory if missing.
4. Write to <path>.tmp.
5. fsync the .tmp file.
6. Rename to <path>.
7. fsync the parent directory.
```

Step 2 is a guard. ft-history's compaction is what updates `state_hash`; passing
an inconsistent record to `write()` is a bug. Storage refuses rather than
silently re-hashing.

### Reads

`read` reads the working tree file. Validates against `ft-core`'s schema and
verifies the hash. Returns `StorageError::Invalid` on schema violation,
`HashMismatch` on hash inconsistency.

`read_at_ref` uses `ft-git::read_file_at_ref` to fetch bytes from a ref and then
runs the same validation pipeline.

### Listing

`list` walks `records/` (or `records/<type>/` if filtered to one kind), reads
just enough of each file to extract envelope fields needed for filtering, and
returns matching IDs. The full body is fetched on demand by `read`.

For large repositories where `list` becomes expensive, the read-side index
(`ft-index`) is what production code calls. `Storage::list` is used by ft-index
itself during rebuild and by tests.

### Iter

`iter` is the streaming form of `list` + `read`. Used during index rebuild to
avoid loading every record into memory simultaneously.

### Filter semantics

Filters compose with AND. Each field's vector composes with OR among its values:

```
kinds = [Task, Bug]
statuses = [Open, Ready]
```

matches a record whose kind is Task OR Bug, AND whose status is Open OR Ready.

### Path resolution

`path_for(id)` is pure (no I/O) and returns the canonical path. Used by callers
that need to know the path without reading the file.

### Concurrency

Two writers attempting to write the same record race on the rename. The OS
guarantees rename atomicity; the loser sees the winner's content on subsequent
reads. Higher layers handle the semantics of "should this write have raced?" —
ft-storage just makes sure the file is never half-written.

---

## Acceptance

1. Round-trip: build a `Task` via `ft-testkit::make_task()`, write it, read it,
   assert all fields identical including `state_hash` and `prev_state_hash`.
2. Atomic write: simulate a crash between `.tmp` write and rename by killing
   the test process; on reopen, the previous version (or no file) is readable
   and the workspace is consistent. (Use a fault-injection layer in tests.)
3. `read_at_ref` retrieves a record from a named branch while checked out on a
   different branch.
4. `list` with each filter combination returns the expected subset.
5. Two threads writing different records (different IDs) in parallel do not
   corrupt either file.
6. Two threads writing the same record produce a single resulting file matching
   one of the two writes (last-rename-wins).
7. Hash-mismatch path: write a record, manually alter its file body without
   updating `state_hash`, attempt read, assert `StorageError::HashMismatch`.
8. Schema-violation path: write garbage to a record file, attempt read, assert
   `StorageError::Invalid` with a clear reason.

---

## Testing requirements

- Unit tests for `StorageFilter` composition.
- Property tests for path encoding: `path_for` is the inverse of parsing the path
  back to `(kind, id)`.
- Integration tests using `ft-testkit::TestRepo` for read/write/list flows.
- Crash-injection tests for atomicity (use a fault layer that panics between
  fsync and rename, then assert recovery).
- Doc tests on every public method.

---

## Out of scope

- External storage mode (ADR-0006). Added at M5. The `Storage` trait is what we
  re-implement; `EmbeddedStorage`'s implementation is replaced, not extended.
- The custom JSON merge driver — that lives in `ft-pr` (M4).
- Index updates (the index is a derived cache; `ft-index` watches storage paths).

---

## References

- ADR-0002 — JSON-in-Git storage
- ADR-0006 — Storage modes
- ADR-0015 — Hash-based IDs (filename convention)
- ADR-0017 — Audit chain integrity (state_hash verification on write)
