//! Pure resolution for per-scope repo profiles: `merge` (base ⊕ delta),
//! `scope_for_path` (last-declared-wins), and `validate_plan` (changeset →
//! distinct validate commands). No storage/IO here — callers pass bodies in.
//!
//! Design: `docs/specs/2026-05-31-per-scope-profiles-design.md`.

use ft_core::RepoProfileBody;

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
}
