//! Slow-path perf fixture for `ft-index`.
//!
//! Gated behind the `slow-tests` cargo feature so the default
//! `cargo test -p ft-index` invocation stays cheap. Run explicitly via:
//!
//! ```text
//! cargo test -p ft-index --features slow-tests --test perf
//! ```
//!
//! Builds a ~1000-record fixture mixing `Task`, `Epic`, `Subtask`, and `Bug`
//! records (with realistic `parent_epic` / `child_ids` edges) and asserts that
//! `rebuild_from` plus a handful of representative queries stay inside
//! generous wall-clock budgets. The actual elapsed time is printed via
//! `eprintln!` so future reviewers can eyeball the trend.

#![cfg(feature = "slow-tests")]
#![allow(
    missing_docs,
    clippy::similar_names,
    clippy::cast_possible_truncation,
    clippy::cloned_ref_to_slice_refs,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ft_core::{Record, RecordBody, RecordId};
use ft_index::{Index, ListQuery, OrderBy, ReadyQuery, Storage, StorageError, StorageFilter};
use ft_testkit::{make_bug, make_epic, make_subtask, make_task};
use tempfile::TempDir;

// ─── Minimal in-memory storage backend ──────────────────────────────────────
//
// Duplicated from `tests/integration.rs` on purpose: extracting a shared
// helper would force a `pub` test-only module and is scope creep for the
// perf gate. The shim implements the full `ft_storage::Storage` trait so it
// can stand in for `EmbeddedStorage`; only the read-side methods are
// exercised here.

struct MemStorage {
    root: PathBuf,
    inner: Mutex<Vec<Record>>,
}

impl MemStorage {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Mutex::new(Vec::new()),
        }
    }

    fn insert(&self, record: Record) {
        let mut g = self.inner.lock().unwrap();
        g.retain(|r| r.envelope.id != record.envelope.id);
        g.push(record);
    }

    fn records_dir(&self) -> PathBuf {
        self.root.join(".firetrail").join("records")
    }
}

impl Storage for MemStorage {
    fn read(&self, id: &RecordId) -> Result<Record, StorageError> {
        let g = self.inner.lock().unwrap();
        g.iter()
            .find(|r| &r.envelope.id == id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(id.clone()))
    }

    fn read_at_ref(&self, _gitref: &str, id: &RecordId) -> Result<Record, StorageError> {
        self.read(id)
    }

    fn write(&self, _record: &Record) -> Result<PathBuf, StorageError> {
        unimplemented!("MemStorage::write is not exercised by ft-index perf test")
    }

    fn delete(&self, id: &RecordId) -> Result<(), StorageError> {
        let mut g = self.inner.lock().unwrap();
        let pos = g
            .iter()
            .position(|r| &r.envelope.id == id)
            .ok_or_else(|| StorageError::NotFound(id.clone()))?;
        g.remove(pos);
        Ok(())
    }

    fn list(&self, _filter: &StorageFilter) -> Result<Vec<RecordId>, StorageError> {
        let g = self.inner.lock().unwrap();
        Ok(g.iter().map(|r| r.envelope.id.clone()).collect())
    }

    fn iter<'a>(
        &'a self,
        _filter: &'a StorageFilter,
    ) -> Box<dyn Iterator<Item = Result<Record, StorageError>> + 'a> {
        let g = self.inner.lock().unwrap();
        let snap: Vec<Record> = g.clone();
        Box::new(snap.into_iter().map(Ok))
    }

    fn path_for(&self, id: &RecordId) -> PathBuf {
        let kind_str = match id.kind() {
            ft_core::RecordKind::Task => "task",
            ft_core::RecordKind::Epic => "epic",
            ft_core::RecordKind::Subtask => "subtask",
            ft_core::RecordKind::Bug => "bug",
            ft_core::RecordKind::Incident => "incident",
            ft_core::RecordKind::Finding => "finding",
            ft_core::RecordKind::Runbook => "runbook",
            ft_core::RecordKind::Decision => "decision",
            ft_core::RecordKind::Gotcha => "gotcha",
            ft_core::RecordKind::Memory => "memory",
        };
        self.records_dir()
            .join(kind_str)
            .join(format!("{}.json", id.as_str().to_lowercase()))
    }

    fn records_root(&self) -> PathBuf {
        self.records_dir()
    }
}

// ─── Perf test ─────────────────────────────────────────────────────────────

/// Rebuild + query budget on a 1000-record fixture.
///
/// Budgets are intentionally generous — this is a regression tripwire, not a
/// micro-benchmark. The actual elapsed times are printed to stderr.
#[test]
fn rebuild_and_query_1000_records_within_budget() {
    const NUM_EPICS: usize = 100;
    const CHILDREN_PER_EPIC_MIN: usize = 5;
    const CHILDREN_PER_EPIC_MAX: usize = 7;
    const NUM_TASKS: usize = 700;
    const NUM_SUBTASKS: usize = 150;
    const NUM_BUGS: usize = 50;

    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".firetrail").join("records")).unwrap();
    let mut idx = Index::open(dir.path()).unwrap();
    let storage = MemStorage::new(dir.path().to_path_buf());

    // 1. Build epics first so we have ids to attach tasks to.
    let mut epics: Vec<Record> = (0..NUM_EPICS)
        .map(|i| make_epic().title(format!("epic-{i:03}")).build())
        .collect();

    // 2. Build tasks; assign the first chunk to epics (5-7 per epic) so
    //    parent_epic/child_ids relations populate, and leave the remainder
    //    as orphan tasks.
    let mut tasks: Vec<Record> = Vec::with_capacity(NUM_TASKS);
    let mut task_cursor = 0usize;
    for (epic_idx, epic) in epics.iter_mut().enumerate() {
        // Deterministic spread: cycle 5,6,7,5,6,7,…
        let n = CHILDREN_PER_EPIC_MIN
            + (epic_idx % (CHILDREN_PER_EPIC_MAX - CHILDREN_PER_EPIC_MIN + 1));
        for child_i in 0..n {
            if task_cursor >= NUM_TASKS {
                break;
            }
            let t = make_task()
                .title(format!("task-{task_cursor:04}-of-epic-{epic_idx:03}"))
                .parent_epic(epic.envelope.id.clone())
                .build();
            // Reflect the child link on the epic body and rehash.
            if let RecordBody::Epic(e) = &mut epic.body {
                e.child_ids.push(t.envelope.id.clone());
            }
            tasks.push(t);
            task_cursor += 1;
            let _ = child_i;
        }
    }
    // Orphan tasks (no parent_epic) to fill out to NUM_TASKS.
    while tasks.len() < NUM_TASKS {
        let i = tasks.len();
        tasks.push(make_task().title(format!("task-orphan-{i:04}")).build());
    }
    // Rehash epics now that child_ids are populated.
    for epic in &mut epics {
        epic.envelope.state_hash = String::new();
        epic.envelope.state_hash = ft_core::state_hash(epic).expect("rehash epic");
    }

    // 3. Subtasks: parent each subtask on a task (round-robin across the
    //    epic-owned tasks so we get a third level of hierarchy).
    let subtasks: Vec<Record> = (0..NUM_SUBTASKS)
        .map(|i| {
            let parent = &tasks[i % tasks.len()];
            make_subtask(parent.envelope.id.clone())
                .title(format!("sub-{i:04}"))
                .build()
        })
        .collect();

    // 4. Bugs (no parents — keeps fixture variety honest).
    let bugs: Vec<Record> = (0..NUM_BUGS)
        .map(|i| make_bug().title(format!("bug-{i:03}")).build())
        .collect();

    // 5. Insert everything into the in-memory storage.
    for r in epics
        .iter()
        .chain(tasks.iter())
        .chain(subtasks.iter())
        .chain(bugs.iter())
    {
        storage.insert(r.clone());
    }
    let total = NUM_EPICS + NUM_TASKS + NUM_SUBTASKS + NUM_BUGS;
    assert_eq!(total, 1000, "fixture should be exactly 1000 records");

    // ─── rebuild_from budget ───────────────────────────────────────────
    let rebuild_budget = Duration::from_secs(10);
    let t0 = Instant::now();
    let report = idx.rebuild_from(&storage).unwrap();
    let rebuild_elapsed = t0.elapsed();
    eprintln!(
        "perf: rebuild_from indexed {} records, {} relations in {:?}",
        report.records_indexed, report.relations_indexed, rebuild_elapsed
    );
    assert_eq!(report.records_indexed as usize, total);
    assert!(
        rebuild_elapsed < rebuild_budget,
        "rebuild_from took {rebuild_elapsed:?}, budget was {rebuild_budget:?}"
    );

    // ─── query budgets ────────────────────────────────────────────────
    let query_budget = Duration::from_millis(200);

    let t = Instant::now();
    let listed = idx
        .list(&ListQuery {
            order_by: OrderBy::Priority,
            ..Default::default()
        })
        .unwrap();
    let list_elapsed = t.elapsed();
    eprintln!(
        "perf: list(priority) -> {} rows in {:?}",
        listed.len(),
        list_elapsed
    );
    assert!(
        list_elapsed < query_budget,
        "list took {list_elapsed:?}, budget was {query_budget:?}"
    );
    assert!(!listed.is_empty());

    let t = Instant::now();
    let ready = idx.ready(&ReadyQuery::default()).unwrap();
    let ready_elapsed = t.elapsed();
    eprintln!(
        "perf: ready() -> {} rows in {:?}",
        ready.len(),
        ready_elapsed
    );
    assert!(
        ready_elapsed < query_budget,
        "ready took {ready_elapsed:?}, budget was {query_budget:?}"
    );

    // show() on a known id (pick the first epic).
    let known_id = epics[0].envelope.id.clone();
    let t = Instant::now();
    let shown = idx.show(&known_id).unwrap();
    let show_elapsed = t.elapsed();
    eprintln!("perf: show({known_id:?}) in {show_elapsed:?}");
    assert!(
        show_elapsed < query_budget,
        "show took {show_elapsed:?}, budget was {query_budget:?}"
    );
    assert_eq!(shown.id, known_id);
}
