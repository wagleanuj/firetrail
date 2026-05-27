//! Assertion helpers used by integration tests and the scenario runner.
//!
//! These helpers read and write records as JSON files using the path layout
//! defined in [`crate::paths`]. They will delegate through `ft-storage` once
//! that crate lands; until then, they mirror the documented layout
//! conservatively.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ft_core::{Record, RecordId, Relation, RelationKind, state_hash};
use serde::de::DeserializeOwned;

use crate::paths::record_path;
use crate::repo::TestRepo;

/// Read a record from disk. Panics with a useful message on failure.
fn read_record(repo: &TestRepo, id: &RecordId) -> Record {
    let path = record_path(repo.root(), id);
    let bytes = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "assert_record_exists: failed to read {} (id={id}): {e}\n--- workspace dump ---\n{}",
            path.display(),
            dump_workspace_string(repo),
        )
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "assert_record_exists: invalid JSON at {} (id={id}): {e}",
            path.display()
        )
    })
}

/// Write a [`Record`] to disk under the canonical path. Used by tests and the
/// scenario runner to set up state for assertion helpers.
///
/// Delegates through [`ft_storage::EmbeddedStorage::write`] so the write is
/// atomic and the embedded `state_hash` is verified before the bytes hit
/// disk — tests get the same hash-consistency guarantees as production code
/// paths for free.
///
/// # Errors
///
/// Returns the underlying I/O, serde, or storage error on failure.
pub fn write_record(repo: &TestRepo, record: &Record) -> Result<(), crate::TestKitError> {
    use ft_storage::{EmbeddedStorage, Storage as _};

    // Ensure the records tree exists (TestRepo::new bootstraps it, but
    // belt-and-braces for hand-rolled TestRepoConfig users).
    let records_root = repo.root().join(ft_storage::RECORDS_DIR);
    if !records_root.exists() {
        fs::create_dir_all(&records_root)?;
    }
    let storage = EmbeddedStorage::open(repo.root())
        .map_err(|e| crate::TestKitError::Other(format!("open storage: {e}")))?;
    storage
        .write(record)
        .map_err(|e| crate::TestKitError::Other(format!("write record: {e}")))?;
    Ok(())
}

/// Assert a record file exists at the canonical path for `id`.
///
/// Panics with a workspace dump on failure.
pub fn assert_record_exists(repo: &TestRepo, id: &RecordId) {
    let path = record_path(repo.root(), id);
    assert!(
        path.exists(),
        "expected record file at {} (id={id}) but it does not exist\n--- workspace dump ---\n{}",
        path.display(),
        dump_workspace_string(repo),
    );
}

/// Assert a record's top-level field matches `expected`.
///
/// `field` is looked up first in the envelope then the body. Use dotted paths
/// (e.g. `"body.description"`) to address nested fields explicitly.
///
/// Panics with a useful message on mismatch.
#[allow(clippy::needless_pass_by_value)]
pub fn assert_field<T: DeserializeOwned + PartialEq + std::fmt::Debug>(
    repo: &TestRepo,
    id: &RecordId,
    field: &str,
    expected: T,
) {
    let record = read_record(repo, id);
    let value = serde_json::to_value(&record).expect("Record always serializes");

    let actual_json = resolve_field(&value, field).unwrap_or_else(|| {
        panic!(
            "assert_field: field `{field}` not found on record {id}\nrecord = {value:#}\n\
             --- workspace dump ---\n{}",
            dump_workspace_string(repo),
        )
    });

    let actual: T = serde_json::from_value(actual_json.clone()).unwrap_or_else(|e| {
        panic!(
            "assert_field: cannot decode `{field}` ({actual_json}) as expected type: {e}\n\
             --- workspace dump ---\n{}",
            dump_workspace_string(repo),
        )
    });

    assert!(
        actual == expected,
        "assert_field: record {id} field `{field}` mismatch\n  expected: {expected:?}\n  \
         actual:   {actual:?}\n--- workspace dump ---\n{}",
        dump_workspace_string(repo),
    );
}

fn resolve_field(value: &serde_json::Value, field: &str) -> Option<serde_json::Value> {
    if field.contains('.') {
        let mut current = value;
        for part in field.split('.') {
            current = current.get(part)?;
        }
        return Some(current.clone());
    }
    // Try envelope first, then body, then top-level.
    value
        .get("envelope")
        .and_then(|e| e.get(field))
        .or_else(|| value.get("body").and_then(|b| b.get(field)))
        .or_else(|| value.get(field))
        .cloned()
}

/// Assert that a relation exists between `from` and `to` with the given kind.
///
/// Relations are stored as `relations.json` in `.firetrail/` (an interim
/// layout that ft-storage will own). Each entry is a [`Relation`] JSON.
pub fn assert_relation(repo: &TestRepo, from: &RecordId, to: &RecordId, kind: RelationKind) {
    let path = repo.firetrail_dir().join("relations.json");
    let bytes = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "assert_relation: failed to read {}: {e}\n--- workspace dump ---\n{}",
            path.display(),
            dump_workspace_string(repo),
        )
    });
    let relations: Vec<Relation> = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "assert_relation: invalid relations.json: {e}\n--- workspace dump ---\n{}",
            dump_workspace_string(repo),
        )
    });
    let found = relations
        .iter()
        .any(|r| &r.from == from && &r.to == to && r.kind == kind);
    assert!(
        found,
        "assert_relation: no relation {from} --{kind:?}--> {to}\n  relations = {relations:#?}\n\
         --- workspace dump ---\n{}",
        dump_workspace_string(repo),
    );
}

/// Assert that the on-disk record's `state_hash` matches the canonical hash of
/// its current body+envelope.
pub fn assert_hash_consistent(repo: &TestRepo, id: &RecordId) {
    let record = read_record(repo, id);
    let recomputed =
        state_hash(&record).expect("state_hash recomputation failed (record well-formed)");
    assert!(
        record.envelope.state_hash == recomputed,
        "assert_hash_consistent: record {id}\n  stored:      {}\n  recomputed:  {recomputed}\n\
         --- workspace dump ---\n{}",
        record.envelope.state_hash,
        dump_workspace_string(repo),
    );
}

/// Pretty-print the workspace to stderr for debugging in test failures.
pub fn dump_workspace(repo: &TestRepo) {
    eprintln!("{}", dump_workspace_string(repo));
}

/// String form of [`dump_workspace`] used internally by panic messages.
#[must_use]
pub fn dump_workspace_string(repo: &TestRepo) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "TestRepo @ {}", repo.root().display());
    dump_dir(&mut out, repo.firetrail_dir().as_path(), 0);
    out
}

fn dump_dir(out: &mut String, dir: &Path, depth: usize) {
    let indent = "  ".repeat(depth);
    let Ok(entries) = fs::read_dir(dir) else {
        let _ = writeln!(out, "{indent}<unreadable: {}>", dir.display());
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let path = entry.path();
        if path.is_dir() {
            let _ = writeln!(out, "{indent}{}/", name.to_string_lossy());
            dump_dir(out, &path, depth + 1);
        } else {
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let _ = writeln!(out, "{indent}{} ({size} bytes)", name.to_string_lossy());
        }
    }
}
