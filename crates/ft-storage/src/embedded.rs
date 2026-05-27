//! Embedded storage mode: records live under `<repo>/.firetrail/records/`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ft_core::{Record, RecordId, RecordKind, Relation, state_hash, validate_record_json};
use ft_git::Repo;

use crate::filter::StorageFilter;
use crate::storage::Storage;
use crate::{RECORDS_DIR, StorageError, kind_dir};

/// Embedded-mode storage: records colocated with the working git repository.
///
/// `EmbeddedStorage` keeps an [`Arc<Repo>`] for [`Storage::read_at_ref`].
/// Cloning the struct is cheap (one `Arc` bump + a `PathBuf` clone).
///
/// # Examples
///
/// ```
/// use ft_storage::{EmbeddedStorage, Storage, StorageFilter};
/// use ft_testkit::{TestRepo, make_task};
///
/// let tr = TestRepo::new().unwrap();
/// let storage = EmbeddedStorage::open(tr.root()).unwrap();
/// let record = make_task().title("hello").build();
/// storage.write(&record).unwrap();
/// let back = storage.read(&record.envelope.id).unwrap();
/// assert_eq!(back, record);
/// let ids = storage.list(&StorageFilter::default()).unwrap();
/// assert_eq!(ids, vec![record.envelope.id]);
/// ```
#[derive(Debug, Clone)]
pub struct EmbeddedStorage {
    repo_root: PathBuf,
    git: Arc<Repo>,
}

impl EmbeddedStorage {
    /// Open storage rooted at the given Firetrail workspace.
    ///
    /// Verifies that `.firetrail/records/` exists. Use [`Self::init`] to
    /// bootstrap an empty layout.
    ///
    /// # Errors
    ///
    /// - [`StorageError::NotInitialized`] if `.firetrail/records/` is missing.
    /// - [`StorageError::Git`] if the path is not a git repository.
    pub fn open(repo_root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let repo_root = repo_root.into();
        let records = repo_root.join(RECORDS_DIR);
        if !records.is_dir() {
            return Err(StorageError::NotInitialized(records));
        }
        let git = Repo::open(&repo_root)?;
        Ok(Self {
            repo_root,
            git: Arc::new(git),
        })
    }

    /// Initialize an empty records tree at `.firetrail/records/<type>/` and
    /// open it.
    ///
    /// Creates every per-kind subdirectory so callers can `git add` them
    /// even before any record is written (gitkeep is the caller's choice).
    ///
    /// # Errors
    ///
    /// - [`StorageError::Io`] on filesystem failure.
    /// - [`StorageError::Git`] if the path is not a git repository.
    pub fn init(repo_root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let repo_root = repo_root.into();
        let records = repo_root.join(RECORDS_DIR);
        fs::create_dir_all(&records)?;
        for kind in ALL_KINDS {
            fs::create_dir_all(records.join(kind_dir(*kind)))?;
        }
        let git = Repo::open(&repo_root)?;
        Ok(Self {
            repo_root,
            git: Arc::new(git),
        })
    }

    /// Absolute repo root.
    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Build the relative path (within the repo) of the file backing `id`.
    fn relative_path(id: &RecordId) -> PathBuf {
        let lower = id.as_str().to_lowercase();
        PathBuf::from(RECORDS_DIR)
            .join(kind_dir(id.kind()))
            .join(format!("{lower}.json"))
    }
}

/// Every record kind, in stable order for `init`/`list` iteration.
const ALL_KINDS: &[RecordKind] = &[
    RecordKind::Task,
    RecordKind::Epic,
    RecordKind::Subtask,
    RecordKind::Bug,
    RecordKind::Incident,
    RecordKind::Finding,
    RecordKind::Runbook,
    RecordKind::Decision,
    RecordKind::Gotcha,
    RecordKind::Memory,
];

impl Storage for EmbeddedStorage {
    fn read(&self, id: &RecordId) -> Result<Record, StorageError> {
        let path = self.path_for(id);
        if !path.exists() {
            return Err(StorageError::NotFound(id.clone()));
        }
        let bytes = fs::read(&path)?;
        parse_and_validate(&path, &bytes)
    }

    fn read_at_ref(&self, gitref: &str, id: &RecordId) -> Result<Record, StorageError> {
        let rel = Self::relative_path(id);
        let bytes = match self.git.read_file_at_ref(gitref, &rel) {
            Ok(b) => b,
            Err(ft_git::GitError::FileNotInTree(_, _)) => {
                return Err(StorageError::NotFound(id.clone()));
            }
            Err(e) => return Err(StorageError::Git(e)),
        };
        // Use the absolute working-tree path in error messages so users can
        // locate the file even when the read came from a ref.
        let path = self.repo_root.join(&rel);
        parse_and_validate(&path, &bytes)
    }

    fn write(&self, record: &Record) -> Result<PathBuf, StorageError> {
        // Step 2 of the spec: refuse writes where the embedded state_hash is
        // not consistent with the body. Callers must update state_hash before
        // calling write().
        let recomputed = state_hash(record)?;
        if recomputed != record.envelope.state_hash {
            return Err(StorageError::HashMismatch {
                id: record.envelope.id.clone(),
                file_hash: record.envelope.state_hash.clone(),
                recomputed,
            });
        }

        let path = self.path_for(&record.envelope.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_vec_pretty(record)?;
        let tmp = tmp_path(&path);

        {
            let mut f = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }

        // Atomic rename onto the final path.
        fs::rename(&tmp, &path)?;

        // fsync the parent directory so the rename is durable.
        if let Some(parent) = path.parent() {
            sync_dir(parent)?;
        }

        Ok(path)
    }

    fn delete(&self, id: &RecordId) -> Result<(), StorageError> {
        let path = self.path_for(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(id.clone()))
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    fn list(&self, filter: &StorageFilter) -> Result<Vec<RecordId>, StorageError> {
        let mut out = Vec::new();
        for entry in iter_record_paths(&self.records_root(), filter.kinds.as_deref()) {
            let path = entry?;
            let bytes = fs::read(&path)?;
            // Full parse and validate so the filter can read every field.
            // Records are small (<10 KB typical); a partial parse would
            // optimize one common case but complicate the code path. The
            // production read path is `ft-index`, not `list`.
            let record = parse_and_validate(&path, &bytes)?;
            if filter.matches(&record) {
                out.push(record.envelope.id);
            }
        }
        Ok(out)
    }

    fn iter<'a>(
        &'a self,
        filter: &'a StorageFilter,
    ) -> Box<dyn Iterator<Item = Result<Record, StorageError>> + 'a> {
        let walker = iter_record_paths(&self.records_root(), filter.kinds.as_deref());
        Box::new(walker.filter_map(move |entry| match entry {
            Err(e) => Some(Err(e)),
            Ok(path) => match fs::read(&path).map_err(StorageError::Io) {
                Err(e) => Some(Err(e)),
                Ok(bytes) => match parse_and_validate(&path, &bytes) {
                    Err(e) => Some(Err(e)),
                    Ok(record) => {
                        if filter.matches(&record) {
                            Some(Ok(record))
                        } else {
                            None
                        }
                    }
                },
            },
        }))
    }

    fn path_for(&self, id: &RecordId) -> PathBuf {
        self.repo_root.join(Self::relative_path(id))
    }

    fn records_root(&self) -> PathBuf {
        self.repo_root.join(RECORDS_DIR)
    }

    fn relations(&self) -> Result<Vec<Relation>, StorageError> {
        // `ft-cli`'s `link` / `dep` commands still write directly to
        // `.firetrail/relations.jsonl` (append-only JSONL, one `Relation` per
        // line). Reading that file here moves ownership of the relation read
        // path from `ft-index` into `ft-storage`. Promoting writes through the
        // `Storage` trait is tracked as a follow-up.
        let path = self.repo_root.join(".firetrail").join("relations.jsonl");
        read_relations_jsonl(&path)
    }
}

/// Parse `.firetrail/relations.jsonl` (one JSON-encoded [`Relation`] per line).
///
/// Missing files yield an empty vector; malformed lines surface as
/// [`StorageError::Invalid`] with the offending line number.
fn read_relations_jsonl(path: &Path) -> Result<Vec<Relation>, StorageError> {
    use std::io::{BufRead, BufReader};

    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = File::open(path)?;
    let mut out = Vec::new();
    for (lineno, line) in BufReader::new(f).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let rel: Relation = serde_json::from_str(&line).map_err(|e| StorageError::Invalid {
            path: path.to_path_buf(),
            reason: format!("relations.jsonl line {}: {e}", lineno + 1),
        })?;
        out.push(rel);
    }
    Ok(out)
}

/// Build a unique sibling `.tmp` path for an atomic write.
///
/// The suffix encodes process id, thread id, and a monotonic counter so
/// concurrent writers — even those targeting the same final path — never
/// share a temporary file. The temp is renamed onto the final path; the
/// rename is the atomic step that defines the last-writer-wins outcome.
fn tmp_path(final_path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = format!(".{pid}.{tid}.{n}.tmp");
    let mut s = final_path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Walk `records/` (or per-kind subdirs if `kinds` is `Some`) and yield each
/// `*.json` file path. The walker is eager: I/O errors discovered during
/// walking are returned as elements of the iterator.
fn iter_record_paths(
    records_root: &Path,
    kinds: Option<&[RecordKind]>,
) -> Box<dyn Iterator<Item = Result<PathBuf, StorageError>>> {
    // Determine the directories to walk.
    let dirs: Vec<PathBuf> = match kinds {
        Some(kinds) => kinds
            .iter()
            .map(|k| records_root.join(kind_dir(*k)))
            .collect(),
        None => vec![records_root.to_path_buf()],
    };

    // Collect entries up front so the returned iterator is `'static`.
    let mut entries: Vec<Result<PathBuf, StorageError>> = Vec::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir).min_depth(1).follow_links(false) {
            match entry {
                Err(e) => {
                    // walkdir's error wraps an io::Error. Surface as Io.
                    if let Some(io) = e.io_error() {
                        entries.push(Err(StorageError::Io(std::io::Error::new(
                            io.kind(),
                            io.to_string(),
                        ))));
                    } else {
                        entries.push(Err(StorageError::Io(std::io::Error::other(e.to_string()))));
                    }
                }
                Ok(e) => {
                    let path = e.path();
                    if e.file_type().is_file()
                        && path.extension().is_some_and(|x| x == "json")
                        && !path
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().ends_with(".tmp"))
                    {
                        entries.push(Ok(path.to_path_buf()));
                    }
                }
            }
        }
    }
    Box::new(entries.into_iter())
}

/// fsync a directory so a prior rename is durable on crash.
#[cfg(unix)]
fn sync_dir(path: &Path) -> Result<(), StorageError> {
    let dir = File::open(path)?;
    dir.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> Result<(), StorageError> {
    // Directory fsync is unavailable / not meaningful on non-Unix targets.
    Ok(())
}

/// Parse JSON bytes into a [`Record`], validate against the schema, then
/// verify `state_hash`. Returns rich [`StorageError`] variants on failure.
fn parse_and_validate(path: &Path, bytes: &[u8]) -> Result<Record, StorageError> {
    // First parse to Value for the schema check (so the validator can produce
    // pointer-style errors), then re-parse to Record for typed access.
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| StorageError::Invalid {
            path: path.to_path_buf(),
            reason: format!("malformed json: {e}"),
        })?;

    if let Err(e) = validate_record_json(&value) {
        return Err(StorageError::Invalid {
            path: path.to_path_buf(),
            reason: format!("schema: {e}"),
        });
    }

    let record: Record = serde_json::from_value(value).map_err(|e| StorageError::Invalid {
        path: path.to_path_buf(),
        reason: format!("typed parse: {e}"),
    })?;

    let recomputed = state_hash(&record)?;
    if recomputed != record.envelope.state_hash {
        return Err(StorageError::HashMismatch {
            id: record.envelope.id.clone(),
            file_hash: record.envelope.state_hash.clone(),
            recomputed,
        });
    }

    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_core::{RecordKind, Status};
    use ft_testkit::{TestRepo, make_bug, make_epic, make_task};

    fn open_storage(tr: &TestRepo) -> EmbeddedStorage {
        EmbeddedStorage::open(tr.root()).expect("open")
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().title("hello").build();
        let path = s.write(&r).unwrap();
        assert!(path.exists());
        let back = s.read(&r.envelope.id).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.envelope.state_hash, r.envelope.state_hash);
        assert_eq!(back.envelope.prev_state_hash, r.envelope.prev_state_hash);
    }

    #[test]
    fn path_for_uses_lowercase_id() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().build();
        let path = s.path_for(&r.envelope.id);
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let lower = format!("{}.json", r.envelope.id.as_str().to_lowercase());
        assert_eq!(fname, lower);
        // No uppercase in filename even though the canonical id has uppercase
        // prefix.
        assert!(!fname.chars().any(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn read_missing_returns_not_found() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().build();
        match s.read(&r.envelope.id).unwrap_err() {
            StorageError::NotFound(id) => assert_eq!(id, r.envelope.id),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn open_rejects_uninitialized_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        // Must be a git repo for the Repo::open call to succeed later, but
        // we want to assert NotInitialized fires first, so don't even run
        // `git init` — assert NotInitialized.
        let err = EmbeddedStorage::open(dir.path()).unwrap_err();
        assert!(matches!(err, StorageError::NotInitialized(_)));
    }

    #[test]
    fn init_creates_per_kind_dirs() {
        let dir = tempfile::TempDir::new().unwrap();
        // init bootstraps the records tree but still requires a git repo.
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        let s = EmbeddedStorage::init(dir.path()).unwrap();
        for k in ALL_KINDS {
            assert!(s.records_root().join(kind_dir(*k)).is_dir());
        }
    }

    #[test]
    fn write_rejects_hash_mismatch() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let mut r = make_task().build();
        r.envelope.state_hash =
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into();
        match s.write(&r).unwrap_err() {
            StorageError::HashMismatch { id, .. } => assert_eq!(id, r.envelope.id),
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }

    #[test]
    fn read_detects_tampered_hash() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().title("orig").build();
        s.write(&r).unwrap();

        // Tamper: rewrite the file body but leave state_hash unchanged.
        let path = s.path_for(&r.envelope.id);
        let bytes = std::fs::read(&path).unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["envelope"]["title"] = serde_json::json!("tampered");
        std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

        match s.read(&r.envelope.id).unwrap_err() {
            StorageError::HashMismatch { id, .. } => assert_eq!(id, r.envelope.id),
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }

    #[test]
    fn read_detects_schema_violation() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().build();
        let path = s.path_for(&r.envelope.id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not valid json }").unwrap();
        match s.read(&r.envelope.id).unwrap_err() {
            StorageError::Invalid { reason, .. } => assert!(reason.contains("json")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn delete_removes_file_and_then_reports_not_found() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().build();
        s.write(&r).unwrap();
        s.delete(&r.envelope.id).unwrap();
        assert!(!s.path_for(&r.envelope.id).exists());
        // Second delete -> NotFound
        assert!(matches!(
            s.delete(&r.envelope.id).unwrap_err(),
            StorageError::NotFound(_)
        ));
    }

    #[test]
    fn list_filters_by_kind() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let t = make_task().build();
        let b = make_bug().build();
        let e = make_epic().build();
        s.write(&t).unwrap();
        s.write(&b).unwrap();
        s.write(&e).unwrap();

        let all = s.list(&StorageFilter::default()).unwrap();
        assert_eq!(all.len(), 3);

        let only_tasks = s
            .list(&StorageFilter::default().kind(RecordKind::Task))
            .unwrap();
        assert_eq!(only_tasks, vec![t.envelope.id.clone()]);

        let tasks_and_bugs = s
            .list(
                &StorageFilter::default()
                    .kind(RecordKind::Task)
                    .kind(RecordKind::Bug),
            )
            .unwrap();
        assert_eq!(tasks_and_bugs.len(), 2);
    }

    #[test]
    fn list_filters_by_status() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let open = make_task().status(Status::Open).build();
        let ready = make_task().status(Status::Ready).build();
        s.write(&open).unwrap();
        s.write(&ready).unwrap();
        let only_ready = s
            .list(&StorageFilter::default().status(Status::Ready))
            .unwrap();
        assert_eq!(only_ready, vec![ready.envelope.id]);
    }

    #[test]
    fn iter_streams_records() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        for i in 0..3 {
            let r = make_task().title(format!("t{i}")).build();
            s.write(&r).unwrap();
        }
        let f = StorageFilter::default();
        let got: Vec<_> = s.iter(&f).collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn read_at_ref_round_trips_through_git() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        // Write & commit on main.
        let r = make_task().title("on-main").build();
        s.write(&r).unwrap();
        tr.commit_all("add task").unwrap();

        // Make a branch where the file's content differs.
        tr.branch("feat").unwrap();
        tr.checkout("feat").unwrap();
        let mut r2 = r.clone();
        r2.envelope.title = "on-feat".to_string();
        // Re-hash since title changed.
        r2.envelope.state_hash = String::new();
        r2.envelope.state_hash = ft_core::state_hash(&r2).unwrap();
        s.write(&r2).unwrap();
        tr.commit_all("retitle").unwrap();

        // Working tree currently has "on-feat".
        let wt = s.read(&r.envelope.id).unwrap();
        assert_eq!(wt.envelope.title, "on-feat");

        // But on `main` the original title is still there.
        let on_main = s.read_at_ref("main", &r.envelope.id).unwrap();
        assert_eq!(on_main.envelope.title, "on-main");
    }

    #[test]
    fn read_at_ref_missing_path_returns_not_found() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let r = make_task().build();
        // Record never written and never committed: not-in-tree at HEAD.
        match s.read_at_ref("HEAD", &r.envelope.id).unwrap_err() {
            StorageError::NotFound(id) => assert_eq!(id, r.envelope.id),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn parallel_writes_of_distinct_records_do_not_corrupt() {
        use std::sync::Arc;
        use std::thread;

        let tr = TestRepo::new().unwrap();
        let s = Arc::new(open_storage(&tr));
        let records: Vec<_> = (0..8)
            .map(|i| make_task().title(format!("p{i}")).build())
            .collect();

        let mut handles = Vec::new();
        for r in records.clone() {
            let s = s.clone();
            handles.push(thread::spawn(move || s.write(&r).unwrap()));
        }
        for h in handles {
            h.join().unwrap();
        }
        for r in &records {
            let back = s.read(&r.envelope.id).unwrap();
            assert_eq!(back.envelope.title, r.envelope.title);
        }
    }

    #[test]
    fn relations_reads_jsonl_log() {
        use ft_core::{Identity, RelationKind};

        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);

        // Empty / missing log → empty Vec.
        assert!(s.relations().unwrap().is_empty());

        // Write two relations into `.firetrail/relations.jsonl` directly, the
        // same path `ft-cli`'s `link`/`dep` commands append to today.
        let from = make_task().build();
        let to = make_task().build();
        let rel = ft_core::Relation {
            from: from.envelope.id.clone(),
            to: to.envelope.id.clone(),
            kind: RelationKind::BlockedBy,
            created_at: chrono::Utc::now(),
            created_by: Identity::new("alice").unwrap(),
        };
        let log_path = tr.root().join(".firetrail").join("relations.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let line = serde_json::to_string(&rel).unwrap();
        std::fs::write(&log_path, format!("{line}\n\n{line}\n")).unwrap();

        let got = s.relations().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], rel);
        assert_eq!(got[1], rel);
    }

    #[test]
    fn relations_surfaces_malformed_line() {
        let tr = TestRepo::new().unwrap();
        let s = open_storage(&tr);
        let log_path = tr.root().join(".firetrail").join("relations.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(&log_path, b"{ not valid json }\n").unwrap();
        match s.relations().unwrap_err() {
            StorageError::Invalid { reason, .. } => {
                assert!(reason.contains("line 1"), "got: {reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn parallel_writes_of_same_record_yield_one_of_them() {
        use std::sync::Arc;
        use std::thread;

        let tr = TestRepo::new().unwrap();
        let s = Arc::new(open_storage(&tr));

        // Two distinct write attempts targeting the same id (clone, mutate
        // title, re-hash).
        let r1 = make_task().title("variant-a").build();
        let mut r2 = r1.clone();
        r2.envelope.title = "variant-b".to_string();
        r2.envelope.state_hash = String::new();
        r2.envelope.state_hash = ft_core::state_hash(&r2).unwrap();

        let s1 = s.clone();
        let s2 = s.clone();
        let h1 = thread::spawn(move || s1.write(&r1).unwrap());
        let h2 = thread::spawn(move || s2.write(&r2).unwrap());
        let _ = h1.join().unwrap();
        let _ = h2.join().unwrap();

        // Whatever survived must be one of the two known-good variants.
        let id = make_task().build().envelope.id; // dummy: not used
        let _ = id;
        // Re-derive id from one of the records by reading the only file in
        // the task dir.
        let dir = s.records_root().join("task");
        let only: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(only.len(), 1, "exactly one file survived");
        let path = only.into_iter().next().unwrap().unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        let back = parse_and_validate(&path, &bytes).unwrap();
        assert!(matches!(
            back.envelope.title.as_str(),
            "variant-a" | "variant-b"
        ));
    }
}
