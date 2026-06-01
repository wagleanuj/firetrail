//! Pure resolution for per-scope repo profiles: `merge` (base ⊕ delta),
//! `scope_for_path` (last-declared-wins), and `validate_plan` (changeset →
//! distinct validate commands). No storage/IO here — callers pass bodies in.
//!
//! Design: `docs/specs/2026-05-31-per-scope-profiles-design.md`.

use std::path::Path;

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
}
