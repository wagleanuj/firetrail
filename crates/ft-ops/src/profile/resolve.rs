//! Pure resolution for per-scope repo profiles: `merge` (base ⊕ delta),
//! `scope_for_path` (last-declared-wins), and `validate_plan` (changeset →
//! distinct validate commands). No storage/IO here — callers pass bodies in.
//!
//! Design: `docs/specs/2026-05-31-per-scope-profiles-design.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ft_core::RepoProfileBody;
use ft_scope::ScopeRegistry;

/// Merge a per-scope delta over the base profile, member-wins.
///
/// Scalar fields take the delta when `Some`, else inherit base. List fields
/// (`languages`, `package_managers`, `components`) replace base when the delta's
/// list is non-empty, else inherit. `trust` is the delta's own (per-record).
#[must_use]
pub fn merge(base: &RepoProfileBody, delta: &RepoProfileBody) -> RepoProfileBody {
    RepoProfileBody {
        validate_command: delta
            .validate_command
            .clone()
            .or_else(|| base.validate_command.clone()),
        test_command: delta
            .test_command
            .clone()
            .or_else(|| base.test_command.clone()),
        build_command: delta
            .build_command
            .clone()
            .or_else(|| base.build_command.clone()),
        lint_command: delta
            .lint_command
            .clone()
            .or_else(|| base.lint_command.clone()),
        runtime: delta.runtime.clone().or_else(|| base.runtime.clone()),
        notes: delta.notes.clone().or_else(|| base.notes.clone()),
        languages: pick_list(&base.languages, &delta.languages),
        package_managers: pick_list(&base.package_managers, &delta.package_managers),
        components: pick_list(&base.components, &delta.components),
        trust: delta.trust,
    }
}

fn pick_list<T: Clone>(base: &[T], delta: &[T]) -> Vec<T> {
    if delta.is_empty() {
        base.to_vec()
    } else {
        delta.to_vec()
    }
}

/// The scope governing `path`, last-declared-wins (mirrors CODEOWNERS / the
/// `ft-scope` source order). `None` when no scope matches.
#[must_use]
pub fn scope_for_path<'a>(reg: &'a ScopeRegistry, path: &Path) -> Option<&'a ft_scope::Scope> {
    reg.scopes_for_path(path).into_iter().last()
}

/// One distinct validate command in a [`ValidatePlan`], with provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidateEntry {
    /// The validate command to run.
    pub command: String,
    /// Scope ids (sorted, unique) that resolved to this command. Empty = base.
    pub scopes: Vec<String>,
    /// How many changed files resolved to this command.
    pub file_count: usize,
}

/// The set of distinct validate commands a changeset requires.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidatePlan {
    /// Distinct commands, ordered by command string.
    pub entries: Vec<ValidateEntry>,
    /// Changed files whose resolved profile has no validate command.
    pub unresolved: usize,
}

/// Resolve a changeset to the distinct validate commands to run. `scope_delta`
/// yields a scope's stored delta body (or `None`); the caller wires it to
/// `ft_storage::profile_get_for_scope`.
pub fn validate_plan(
    reg: &ScopeRegistry,
    base: &RepoProfileBody,
    paths: &[PathBuf],
    mut scope_delta: impl FnMut(&str) -> Option<RepoProfileBody>,
) -> ValidatePlan {
    // command -> (set of scope ids, file count)
    let mut acc: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
    let mut unresolved = 0usize;
    for path in paths {
        let (resolved, scope_id) = match scope_for_path(reg, path) {
            Some(scope) => match scope_delta(&scope.id) {
                Some(delta) => (merge(base, &delta), Some(scope.id.clone())),
                None => (base.clone(), Some(scope.id.clone())),
            },
            None => (base.clone(), None),
        };
        match resolved.validate_command {
            Some(cmd) => {
                let slot = acc.entry(cmd).or_default();
                if let Some(id) = scope_id {
                    slot.0.insert(id);
                }
                slot.1 += 1;
            }
            None => unresolved += 1,
        }
    }
    let entries = acc
        .into_iter()
        .map(|(command, (scopes, file_count))| ValidateEntry {
            command,
            scopes: scopes.into_iter().collect(),
            file_count,
        })
        .collect();
    ValidatePlan {
        entries,
        unresolved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_core::RepoProfileBody;

    fn base() -> RepoProfileBody {
        RepoProfileBody {
            validate_command: Some("just ci".into()),
            test_command: Some("cargo test".into()),
            languages: vec!["rust".into()],
            ..Default::default()
        }
    }

    #[test]
    fn delta_overrides_scalar_inherits_rest() {
        let delta = RepoProfileBody {
            test_command: Some("pnpm test".into()),
            ..Default::default()
        };
        let m = merge(&base(), &delta);
        assert_eq!(m.validate_command.as_deref(), Some("just ci")); // inherited
        assert_eq!(m.test_command.as_deref(), Some("pnpm test")); // overridden
    }

    #[test]
    fn nonempty_list_replaces_empty_inherits() {
        let delta = RepoProfileBody {
            languages: vec!["typescript".into()],
            ..Default::default()
        };
        assert_eq!(
            merge(&base(), &delta).languages,
            vec!["typescript".to_string()]
        );

        let empty = RepoProfileBody::default(); // languages empty
        assert_eq!(merge(&base(), &empty).languages, vec!["rust".to_string()]);
    }

    #[test]
    fn last_declared_scope_wins() {
        use ft_scope::ScopeRegistry;
        use ft_testkit::TestRepo;
        use std::path::Path;

        let tr = TestRepo::new().unwrap();
        std::fs::create_dir_all(tr.root().join(".firetrail")).unwrap();
        std::fs::write(
            tr.root().join(".firetrail/scopes.yaml"),
            "scopes:\n  - id: all\n    applies_to: [\"**\"]\n  - id: checkout\n    applies_to: [\"apps/checkout/**\"]\n",
        )
        .unwrap();
        let reg = ScopeRegistry::load(tr.root()).unwrap();

        let id = scope_for_path(&reg, Path::new("apps/checkout/main.ts")).map(|s| s.id.clone());
        assert_eq!(id.as_deref(), Some("checkout")); // last-declared of the two matches
        let id2 = scope_for_path(&reg, Path::new("README.md")).map(|s| s.id.clone());
        assert_eq!(id2.as_deref(), Some("all"));
    }

    #[test]
    fn plan_dedupes_and_counts() {
        use ft_scope::ScopeRegistry;
        use ft_testkit::TestRepo;
        use std::path::PathBuf;

        let tr = TestRepo::new().unwrap();
        std::fs::create_dir_all(tr.root().join(".firetrail")).unwrap();
        std::fs::write(
            tr.root().join(".firetrail/scopes.yaml"),
            "scopes:\n  - id: checkout\n    applies_to: [\"apps/checkout/**\"]\n",
        )
        .unwrap();
        let reg = ScopeRegistry::load(tr.root()).unwrap();

        let base = RepoProfileBody {
            validate_command: Some("just ci".into()),
            ..Default::default()
        };
        let checkout = RepoProfileBody {
            validate_command: Some("pnpm --filter checkout test".into()),
            ..Default::default()
        };

        let paths = vec![
            PathBuf::from("apps/checkout/a.ts"),
            PathBuf::from("apps/checkout/b.ts"),
            PathBuf::from("README.md"),
        ];
        let plan = validate_plan(&reg, &base, &paths, |id| {
            if id == "checkout" {
                Some(checkout.clone())
            } else {
                None
            }
        });
        // two distinct commands: checkout's (2 files) + base's (1 file)
        assert_eq!(plan.entries.len(), 2);
        let checkout_entry = plan
            .entries
            .iter()
            .find(|e| e.command.contains("checkout"))
            .unwrap();
        assert_eq!(checkout_entry.file_count, 2);
        assert_eq!(checkout_entry.scopes, vec!["checkout".to_string()]);
        assert_eq!(plan.unresolved, 0);
    }
}
