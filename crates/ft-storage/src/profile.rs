//! Singleton [`RepoProfile`](ft_core::RecordKind::RepoProfile) accessors.
//!
//! A repo holds at most one **base** `RepoProfile` (`owning_scope == None`) plus
//! at most one **per-scope** profile for each distinct `owning_scope` — a small
//! bag of always-read facts (validate/test/build/lint commands, languages,
//! components, …). These free helpers read and upsert those records through the
//! [`Storage`] trait, so they work against any backend (embedded or external)
//! without duplicating the per-backend write/commit machinery.
//!
//! Design: `docs/specs/2026-05-31-repo-profile-bootstrap-design.md`.
//!
//! ## Convention
//!
//! [`profile_set`] follows the same write convention as the rest of
//! `ft-storage`: it mutates the record body, recomputes `state_hash`, and
//! persists via [`Storage::write`]. It does *not* touch `prev_state_hash` — the
//! chain field is populated by `ft-history`'s `write_with_history` path, never
//! by direct writers. This mirrors how every other in-place edit in the
//! codebase persists (the plain `write` path leaves chaining to `ft-history`).

use chrono::Utc;

use ft_core::{
    Identity, Record, RecordBody, RecordBuilder, RecordKind, RepoProfileBody, state_hash,
};

use crate::StorageError;
use crate::filter::StorageFilter;
use crate::storage::Storage;

/// Read the current repo profile record, or `None` if no profile exists.
///
/// A repo holds at most one `RepoProfile`. If more than one is found (a
/// degenerate state `doctor` warns about), the record with the
/// lexicographically smallest id is returned so the result is deterministic.
///
/// # Errors
///
/// - Any error surfaced by [`Storage::list`] / [`Storage::read`] (I/O, parse,
///   schema, or hash failure).
pub fn profile_get(storage: &dyn Storage) -> Result<Option<Record>, StorageError> {
    let mut ids = storage.list(&StorageFilter::default().kind(RecordKind::RepoProfile))?;
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    match ids.into_iter().next() {
        Some(id) => Ok(Some(storage.read(&id)?)),
        None => Ok(None),
    }
}

/// Upsert the singleton repo profile.
///
/// If a profile already exists, its body is replaced with `body` *in place* —
/// the existing record's `id`, `created_by`, and `created_at` are preserved and
/// `updated_at` is bumped to now, so there is exactly one profile file and it
/// keeps its identity across edits. If no profile exists, a new record is
/// created authored by `author`.
///
/// `state_hash` is recomputed before the write; `prev_state_hash` is left
/// untouched (see the module docs).
///
/// Returns the persisted [`Record`].
///
/// # Errors
///
/// - [`StorageError::Core`] if the record fails to build (only possible on
///   first create, e.g. an empty title — not reachable here since the title is
///   a constant).
/// - Any error surfaced by [`Storage::list`] / [`Storage::read`] /
///   [`Storage::write`].
pub fn profile_set(
    storage: &dyn Storage,
    body: RepoProfileBody,
    author: &Identity,
) -> Result<Record, StorageError> {
    if let Some(mut existing) = profile_get(storage)? {
        // Update in place: reuse id/created_by/created_at, swap the body,
        // bump updated_at, recompute the hash.
        existing.body = RecordBody::RepoProfile(body);
        existing.envelope.updated_at = Utc::now();
        existing.envelope.state_hash = String::new();
        existing.envelope.state_hash = state_hash(&existing)?;
        storage.write(&existing)?;
        Ok(existing)
    } else {
        let record = RecordBuilder::new(RecordKind::RepoProfile, PROFILE_TITLE, author.clone())
            .repo_profile(body)
            .build()?;
        storage.write(&record)?;
        Ok(record)
    }
}

/// Read the **base** repo profile (`owning_scope == None`), or `None`.
///
/// In a monorepo the base is the repo-wide profile that per-scope profiles
/// inherit from. Deterministic on the degenerate >1-base state: smallest id.
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`].
pub fn profile_get_base(storage: &dyn Storage) -> Result<Option<Record>, StorageError> {
    let mut bases: Vec<Record> = profile_records(storage)?
        .into_iter()
        .filter(|r| r.envelope.owning_scope.is_none())
        .collect();
    bases.sort_by(|a, b| a.envelope.id.as_str().cmp(b.envelope.id.as_str()));
    Ok(bases.into_iter().next())
}

/// Read the per-scope profile delta for `scope_id` (`owning_scope == Some`), or
/// `None`. Deterministic on duplicates: smallest id.
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`].
pub fn profile_get_for_scope(
    storage: &dyn Storage,
    scope_id: &str,
) -> Result<Option<Record>, StorageError> {
    let mut hits: Vec<Record> = profile_records(storage)?
        .into_iter()
        .filter(|r| r.envelope.owning_scope.as_deref() == Some(scope_id))
        .collect();
    hits.sort_by(|a, b| a.envelope.id.as_str().cmp(b.envelope.id.as_str()));
    Ok(hits.into_iter().next())
}

/// Read every `RepoProfile` record (base + all scopes), id-sorted.
fn profile_records(storage: &dyn Storage) -> Result<Vec<Record>, StorageError> {
    let mut ids = storage.list(&StorageFilter::default().kind(RecordKind::RepoProfile))?;
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    ids.into_iter().map(|id| storage.read(&id)).collect()
}

/// Every `RepoProfile` record (base + per-scope), id-sorted.
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`].
pub fn profile_list(storage: &dyn Storage) -> Result<Vec<Record>, StorageError> {
    profile_records(storage)
}

/// Upsert the per-scope profile delta for `scope_id` in place; create with
/// `owning_scope = Some(scope_id)` if absent. Mirrors [`profile_set`].
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`] / [`Storage::write`],
/// or [`StorageError::Core`] on first build.
pub fn profile_set_for_scope(
    storage: &dyn Storage,
    scope_id: &str,
    body: RepoProfileBody,
    author: &Identity,
) -> Result<Record, StorageError> {
    if let Some(mut existing) = profile_get_for_scope(storage, scope_id)? {
        existing.body = RecordBody::RepoProfile(body);
        existing.envelope.updated_at = Utc::now();
        existing.envelope.state_hash = String::new();
        existing.envelope.state_hash = state_hash(&existing)?;
        storage.write(&existing)?;
        Ok(existing)
    } else {
        let record = RecordBuilder::new(RecordKind::RepoProfile, PROFILE_TITLE, author.clone())
            .owning_scope(scope_id)
            .repo_profile(body)
            .build()?;
        storage.write(&record)?;
        Ok(record)
    }
}

/// Title given to a freshly-created repo profile record.
const PROFILE_TITLE: &str = "Repo profile";

#[cfg(test)]
mod tests {
    use super::*;
    use ft_core::{ComponentRef, TrustState};
    use ft_testkit::{TestRepo, make_identity};

    use crate::EmbeddedStorage;

    fn open(tr: &TestRepo) -> EmbeddedStorage {
        EmbeddedStorage::open(tr.root()).expect("open")
    }

    fn sample_body() -> RepoProfileBody {
        RepoProfileBody {
            validate_command: Some("cargo test && cargo clippy -- -D warnings".into()),
            test_command: Some("cargo test".into()),
            build_command: Some("cargo build".into()),
            lint_command: Some("cargo clippy".into()),
            languages: vec!["rust".into()],
            package_managers: vec!["cargo".into()],
            runtime: Some("rust 1.80".into()),
            components: vec![ComponentRef {
                name: "ft-storage".into(),
                path: "crates/ft-storage".into(),
                summary: Some("storage layer".into()),
            }],
            notes: Some("initial".into()),
            trust: TrustState::Draft,
        }
    }

    fn scope_record(scope: &str, validate: &str) -> Record {
        let body = RepoProfileBody {
            validate_command: Some(validate.into()),
            ..Default::default()
        };
        RecordBuilder::new(RecordKind::RepoProfile, "Repo profile", make_identity())
            .owning_scope(scope)
            .repo_profile(body)
            .build()
            .unwrap()
    }

    #[test]
    fn base_get_ignores_scope_records() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);
        profile_set(&s, sample_body(), &make_identity()).unwrap(); // base
        s.write(&scope_record("apps/checkout", "pnpm test"))
            .unwrap();

        let base = profile_get_base(&s).unwrap().expect("base present");
        assert_eq!(base.envelope.owning_scope, None);
    }

    #[test]
    fn scope_get_matches_owning_scope() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);
        s.write(&scope_record("apps/checkout", "pnpm test"))
            .unwrap();
        s.write(&scope_record("libs/ui", "pnpm --filter ui test"))
            .unwrap();

        let got = profile_get_for_scope(&s, "apps/checkout")
            .unwrap()
            .expect("present");
        assert_eq!(got.envelope.owning_scope.as_deref(), Some("apps/checkout"));
        assert!(profile_get_for_scope(&s, "nope").unwrap().is_none());
    }

    #[test]
    fn list_returns_base_and_scopes() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);
        profile_set(&s, sample_body(), &make_identity()).unwrap();
        s.write(&scope_record("apps/checkout", "pnpm test"))
            .unwrap();
        let all = profile_list(&s).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn set_for_scope_upserts_in_place() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);
        let mut b = RepoProfileBody {
            test_command: Some("pnpm test".into()),
            ..Default::default()
        };
        let first =
            profile_set_for_scope(&s, "apps/checkout", b.clone(), &make_identity()).unwrap();
        assert_eq!(
            first.envelope.owning_scope.as_deref(),
            Some("apps/checkout")
        );

        b.test_command = Some("pnpm --filter checkout test".into());
        let second = profile_set_for_scope(&s, "apps/checkout", b, &make_identity()).unwrap();
        assert_eq!(first.envelope.id, second.envelope.id, "upsert in place");
        assert_eq!(
            profile_get_for_scope(&s, "apps/checkout")
                .unwrap()
                .unwrap()
                .envelope
                .id,
            first.envelope.id
        );
        // base untouched / absent
        assert!(profile_get_base(&s).unwrap().is_none());
    }

    #[test]
    fn get_returns_none_when_absent() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);
        assert!(profile_get(&s).unwrap().is_none());
    }

    #[test]
    fn set_creates_then_round_trips() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);
        let body = sample_body();
        let created = profile_set(&s, body.clone(), &make_identity()).unwrap();

        assert_eq!(created.envelope.kind, RecordKind::RepoProfile);
        match &created.body {
            RecordBody::RepoProfile(b) => assert_eq!(b, &body),
            other => panic!("expected RepoProfile body, got {other:?}"),
        }

        // Round-trips off disk unchanged.
        let back = profile_get(&s).unwrap().expect("profile present");
        assert_eq!(back, created);
        assert_eq!(back.envelope.state_hash, created.envelope.state_hash);

        // Lives under the repo_profile partition.
        let path = s.path_for(&created.envelope.id);
        assert!(
            path.parent().unwrap().ends_with("repo_profile"),
            "path: {path:?}"
        );
    }

    #[test]
    fn set_is_a_true_singleton_update_not_a_duplicate() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);

        let first = profile_set(&s, sample_body(), &make_identity()).unwrap();

        // Second set with different fields.
        let mut second_body = sample_body();
        second_body.validate_command = Some("just ci".into());
        second_body.notes = Some("updated".into());
        let second = profile_set(&s, second_body.clone(), &make_identity()).unwrap();

        // Same record id — updated in place, not a new record.
        assert_eq!(first.envelope.id, second.envelope.id);

        // Exactly one file on disk under records/repo_profile/.
        let dir = s.records_root().join("repo_profile");
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        assert_eq!(files.len(), 1, "exactly one profile file: {files:?}");

        // The persisted body is the second set's fields.
        let back = profile_get(&s).unwrap().expect("profile present");
        match &back.body {
            RecordBody::RepoProfile(b) => assert_eq!(b, &second_body),
            other => panic!("expected RepoProfile body, got {other:?}"),
        }
        assert_eq!(back.envelope.id, first.envelope.id);
    }

    #[test]
    fn set_preserves_created_metadata_and_bumps_updated_at() {
        let tr = TestRepo::new().unwrap();
        let s = open(&tr);

        let first = profile_set(&s, sample_body(), &make_identity()).unwrap();
        let created_by = first.envelope.created_by.clone();
        let created_at = first.envelope.created_at;

        // Backdate created/updated so the bump is observable regardless of
        // clock resolution.
        let mut backdated = first.clone();
        let old = Utc::now() - chrono::Duration::hours(1);
        backdated.envelope.created_at = old;
        backdated.envelope.updated_at = old;
        backdated.envelope.state_hash = String::new();
        backdated.envelope.state_hash = state_hash(&backdated).unwrap();
        s.write(&backdated).unwrap();

        let mut next = sample_body();
        next.notes = Some("changed".into());
        let updated = profile_set(&s, next, &make_identity()).unwrap();

        // created_by / created_at preserved from the existing record.
        assert_eq!(updated.envelope.created_by, created_by);
        let _ = created_at;
        assert_eq!(updated.envelope.created_at, old);
        // updated_at moved forward.
        assert!(updated.envelope.updated_at > old);
    }
}
