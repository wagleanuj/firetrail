//! Integration tests for `ft-index`.
//!
//! These exercise the full open → rebuild → query → refresh path against an
//! in-memory `Storage` implementation built around `ft-testkit` factories.

#![allow(
    missing_docs,
    clippy::similar_names,
    clippy::cast_possible_truncation,
    clippy::cloned_ref_to_slice_refs,
    clippy::doc_markdown
)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use ft_core::{Claim, Identity, Priority, Record, RecordBody, RecordId, Status};
use ft_index::{
    Index, ListQuery, OrderBy, ReadyQuery, Storage, StorageError, StorageFilter, WalkDirection,
};
use ft_testkit::{make_bug, make_epic, make_identity_named, make_subtask, make_task};
use tempfile::TempDir;

// ─── Minimal in-memory storage backend for tests ────────────────────────────

#[derive(Default)]
struct MemStorage {
    inner: Mutex<Vec<(Record, PathBuf)>>,
}

impl MemStorage {
    fn new() -> Self {
        Self::default()
    }

    fn insert(&self, record: Record) -> PathBuf {
        let path = PathBuf::from(format!(".firetrail/records/{}.json", record.envelope.id));
        let mut g = self.inner.lock().unwrap();
        g.retain(|(r, _)| r.envelope.id != record.envelope.id);
        g.push((record, path.clone()));
        path
    }

    fn remove(&self, id: &RecordId) -> Option<PathBuf> {
        let mut g = self.inner.lock().unwrap();
        let pos = g.iter().position(|(r, _)| &r.envelope.id == id)?;
        Some(g.remove(pos).1)
    }
}

impl Storage for MemStorage {
    fn iter(
        &self,
        filter: StorageFilter,
    ) -> Result<Box<dyn Iterator<Item = Result<(Record, PathBuf), StorageError>> + '_>, StorageError>
    {
        let g = self.inner.lock().unwrap();
        let snap: Vec<_> = g
            .iter()
            .filter(|(r, _)| {
                filter.include_closed
                    || !matches!(
                        r.envelope.status,
                        Status::Closed | Status::Deferred | Status::Archived
                    )
            })
            .cloned()
            .collect();
        Ok(Box::new(snap.into_iter().map(Ok)))
    }

    fn read(&self, id: &RecordId) -> Result<(Record, PathBuf), StorageError> {
        let g = self.inner.lock().unwrap();
        g.iter()
            .find(|(r, _)| &r.envelope.id == id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(id.to_string()))
    }

    fn read_path(&self, path: &Path) -> Result<Record, StorageError> {
        let g = self.inner.lock().unwrap();
        g.iter()
            .find(|(_, p)| p == path)
            .map(|(r, _)| r.clone())
            .ok_or_else(|| StorageError::NotFound(path.to_string_lossy().into()))
    }
}

fn fresh_index() -> (TempDir, Index) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".firetrail")).unwrap();
    let idx = Index::open(dir.path()).unwrap();
    (dir, idx)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[test]
fn open_creates_schema_v1() {
    let (_d, idx) = fresh_index();
    assert_eq!(idx.schema_version(), 1);
}

#[test]
fn rebuild_from_empty_storage_succeeds() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let report = idx.rebuild_from(&storage).unwrap();
    assert_eq!(report.records_indexed, 0);
    assert_eq!(report.relations_indexed, 0);
    assert_eq!(idx.list(&ListQuery::default()).unwrap().len(), 0);
}

#[test]
fn rebuild_indexes_all_records() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let t1 = make_task().title("first").build();
    let t2 = make_task().title("second").priority(Priority::P0).build();
    storage.insert(t1.clone());
    storage.insert(t2.clone());

    let report = idx.rebuild_from(&storage).unwrap();
    assert_eq!(report.records_indexed, 2);

    let all = idx.list(&ListQuery::default()).unwrap();
    assert_eq!(all.len(), 2);
    // Priority order: P0 first.
    assert_eq!(all[0].id, t2.envelope.id);
    assert_eq!(all[1].id, t1.envelope.id);
}

#[test]
fn list_filters_compose() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let alice = make_identity_named("alice");
    let bob = make_identity_named("bob");

    let t_alice = make_task().title("a").owner(alice.clone()).build();
    let t_bob = make_task().title("b").owner(bob.clone()).build();
    let bug = make_bug()
        .title("bug")
        .owner(alice.clone())
        .priority(Priority::P0)
        .build();
    storage.insert(t_alice.clone());
    storage.insert(t_bob);
    storage.insert(bug.clone());
    idx.rebuild_from(&storage).unwrap();

    let q = ListQuery {
        owners: Some(vec![alice.clone()]),
        ..Default::default()
    };
    let got = idx.list(&q).unwrap();
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|r| r.owner.as_ref() == Some(&alice)));

    let q2 = ListQuery {
        owners: Some(vec![alice.clone()]),
        kinds: Some(vec![ft_core::RecordKind::Bug]),
        ..Default::default()
    };
    let got2 = idx.list(&q2).unwrap();
    assert_eq!(got2.len(), 1);
    assert_eq!(got2[0].id, bug.envelope.id);
}

#[test]
fn list_excludes_closed_by_default() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let open = make_task().title("open").build();
    let closed = make_task().title("closed").status(Status::Closed).build();
    storage.insert(open.clone());
    storage.insert(closed.clone());
    idx.rebuild_from(&storage).unwrap();

    let visible = idx.list(&ListQuery::default()).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, open.envelope.id);

    let with_closed = idx
        .list(&ListQuery {
            include_closed: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_closed.len(), 2);
}

#[test]
fn count_matches_list_len() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    for i in 0..7 {
        storage.insert(make_task().title(format!("t{i}")).build());
    }
    idx.rebuild_from(&storage).unwrap();
    let q = ListQuery::default();
    assert_eq!(idx.count(&q).unwrap() as usize, idx.list(&q).unwrap().len());
}

#[test]
fn list_filters_by_label() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    storage.insert(make_task().label("area", "search").build());
    storage.insert(make_task().label("area", "index").build());
    idx.rebuild_from(&storage).unwrap();

    let q = ListQuery {
        labels: vec![("area".into(), "search".into())],
        ..Default::default()
    };
    let got = idx.list(&q).unwrap();
    assert_eq!(got.len(), 1);
}

#[test]
fn parent_child_relations_are_derived() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let epic = make_epic().title("E").build();
    let task = make_task()
        .title("T")
        .parent_epic(epic.envelope.id.clone())
        .build();
    let sub = make_subtask(task.envelope.id.clone()).title("S").build();
    storage.insert(epic.clone());
    storage.insert(task.clone());
    storage.insert(sub.clone());

    let report = idx.rebuild_from(&storage).unwrap();
    assert!(report.relations_indexed >= 4); // task↔epic and sub↔task

    let children = idx.child_records(&epic.envelope.id).unwrap();
    assert_eq!(children, vec![task.envelope.id.clone()]);
    let children2 = idx.child_records(&task.envelope.id).unwrap();
    assert_eq!(children2, vec![sub.envelope.id.clone()]);

    let kids_via_list = idx
        .list(&ListQuery {
            parent: Some(epic.envelope.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(kids_via_list.len(), 1);
}

#[test]
fn ready_excludes_blocked_records() {
    use ft_core::Relation;
    use ft_index::IndexError;

    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let blocker = make_task().title("blocker").build();
    let blocked = make_task().title("blocked").build();
    storage.insert(blocker.clone());
    storage.insert(blocked.clone());
    idx.rebuild_from(&storage).unwrap();

    // Both ready initially.
    let ready0 = idx.ready(&ReadyQuery::default()).unwrap();
    assert_eq!(ready0.len(), 2);

    // Inject a blocked-by edge by hand (until ft-storage owns relation writes).
    insert_blocked_by(
        &idx,
        &blocked.envelope.id,
        &blocker.envelope.id,
        &make_identity_named("tester"),
    );

    let ready = idx.ready(&ReadyQuery::default()).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, blocker.envelope.id);

    // Close the blocker → blocked becomes ready.
    let mut blocker_closed = blocker.clone();
    blocker_closed.envelope.status = Status::Closed;
    storage.insert(blocker_closed.clone());
    let path = storage
        .read(&blocker_closed.envelope.id)
        .map(|(_, p)| p)
        .unwrap();
    idx.refresh(&storage, &[path], &[]).unwrap();

    let ready2 = idx.ready(&ReadyQuery::default()).unwrap();
    let visible: Vec<_> = ready2.iter().map(|r| &r.id).collect();
    assert!(visible.contains(&&blocked.envelope.id));
    // `blocker_closed` is closed → not ready.
    assert!(!visible.contains(&&blocker_closed.envelope.id));

    // Sanity: the helper compiled.
    let _: fn(&Index, &RecordId, &RecordId, &Identity) = insert_blocked_by;
    let _: fn(IndexError) -> String = |e| e.to_string();
    let _: Relation; // keep the import meaningful even though we bypass it
}

#[test]
fn ready_excludes_actively_claimed_records() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let alice = make_identity_named("alice");

    // Build a task with an unexpired claim.
    let mut task = make_task().title("claimed").build();
    if let RecordBody::Task(t) = &mut task.body {
        t.claim = Some(Claim {
            claimed_by: alice.clone(),
            claimed_at: Utc::now(),
            claim_source: "test".into(),
            claim_expires_at: Utc::now() + chrono::Duration::hours(1),
        });
    }
    storage.insert(task.clone());
    storage.insert(make_task().title("free").build());
    idx.rebuild_from(&storage).unwrap();

    let ready = idx.ready(&ReadyQuery::default()).unwrap();
    assert_eq!(ready.len(), 1);
    assert_ne!(ready[0].id, task.envelope.id);

    let with_claimed = idx
        .ready(&ReadyQuery {
            include_claimed: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_claimed.len(), 2);
}

#[test]
fn refresh_handles_add_modify_delete() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let t = make_task().title("v1").build();
    let p = storage.insert(t.clone());
    idx.rebuild_from(&storage).unwrap();

    // Modify
    let mut t2 = t.clone();
    t2.envelope.title = "v2".into();
    storage.insert(t2.clone());
    let report = idx.refresh(&storage, &[p.clone()], &[]).unwrap();
    assert_eq!(report.records_updated, 1);
    assert_eq!(idx.show(&t.envelope.id).unwrap().title, "v2");

    // Add
    let t_new = make_task().title("new").build();
    let p_new = storage.insert(t_new.clone());
    let report2 = idx.refresh(&storage, &[p_new.clone()], &[]).unwrap();
    assert_eq!(report2.records_added, 1);

    // Delete
    let removed_path = storage.remove(&t.envelope.id).unwrap();
    let report3 = idx.refresh(&storage, &[], &[removed_path]).unwrap();
    assert_eq!(report3.records_removed, 1);
    assert!(idx.show(&t.envelope.id).is_err());
}

#[test]
fn dependency_walk_handles_cycles() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let a = make_task().title("a").build();
    let b = make_task().title("b").build();
    let c = make_task().title("c").build();
    storage.insert(a.clone());
    storage.insert(b.clone());
    storage.insert(c.clone());
    idx.rebuild_from(&storage).unwrap();

    let tester = make_identity_named("tester");
    // a → b → c → a (cycle on blocked-by)
    insert_blocked_by(&idx, &a.envelope.id, &b.envelope.id, &tester);
    insert_blocked_by(&idx, &b.envelope.id, &c.envelope.id, &tester);
    insert_blocked_by(&idx, &c.envelope.id, &a.envelope.id, &tester);

    let walk = idx
        .dependency_walk(&a.envelope.id, WalkDirection::Upstream, 10)
        .unwrap();
    // Walk should terminate — at most 3 distinct edges.
    assert!(walk.len() <= 3, "walk produced {} edges", walk.len());
    assert!(!walk.is_empty());
}

#[test]
fn show_returns_indexed_record() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let t = make_task().title("hello").build();
    storage.insert(t.clone());
    idx.rebuild_from(&storage).unwrap();

    let got = idx.show(&t.envelope.id).unwrap();
    assert_eq!(got.title, "hello");
    assert_eq!(got.id, t.envelope.id);
}

#[test]
fn relations_returns_both_directions() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    let epic = make_epic().title("e").build();
    let task = make_task()
        .title("t")
        .parent_epic(epic.envelope.id.clone())
        .build();
    storage.insert(epic.clone());
    storage.insert(task.clone());
    idx.rebuild_from(&storage).unwrap();

    let rels = idx.relations(&epic.envelope.id).unwrap();
    assert!(!rels.is_empty());
    assert!(
        rels.iter()
            .any(|e| e.from == task.envelope.id || e.to == task.envelope.id)
    );
}

#[test]
fn list_order_by_title() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    storage.insert(make_task().title("zzz").build());
    storage.insert(make_task().title("aaa").build());
    idx.rebuild_from(&storage).unwrap();

    let q = ListQuery {
        order_by: OrderBy::Title,
        ..Default::default()
    };
    let got = idx.list(&q).unwrap();
    assert_eq!(got[0].title, "aaa");
    assert_eq!(got[1].title, "zzz");
}

#[test]
fn list_limit_and_offset() {
    let (_d, mut idx) = fresh_index();
    let storage = MemStorage::new();
    for i in 0..5 {
        storage.insert(make_task().title(format!("t{i:02}")).build());
    }
    idx.rebuild_from(&storage).unwrap();

    let q = ListQuery {
        order_by: OrderBy::Title,
        limit: Some(2),
        offset: Some(1),
        ..Default::default()
    };
    let got = idx.list(&q).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].title, "t01");
    assert_eq!(got[1].title, "t02");
}

// ─── helpers ───────────────────────────────────────────────────────────────

/// Inject a `blocked-by` edge directly into the index's SQLite database.
///
/// Real callers will write a Relation record via `ft-storage` and trigger a
/// refresh; until that crate lands we exercise the read path by writing the
/// edge through a side channel.
fn insert_blocked_by(idx: &Index, from: &RecordId, to: &RecordId, by: &Identity) {
    let conn = rusqlite::Connection::open(idx.db_path()).unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO relations(from_id, to_id, kind, created_at, created_by)
         VALUES(?1, ?2, 'blocked-by', ?3, ?4)",
        rusqlite::params![
            from.as_str(),
            to.as_str(),
            Utc::now().to_rfc3339(),
            by.as_str(),
        ],
    )
    .unwrap();
}
