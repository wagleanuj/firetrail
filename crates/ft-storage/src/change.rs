//! Path-classification helpers used by CI/commit hooks (ADR-0009).
//!
//! These helpers turn a list of changed paths into a coarse classification —
//! "memory-only" (safe to merge with a relaxed policy under ADR-0009),
//! structural (touches plannable records like Task / Epic / Bug / Subtask),
//! config (the `.firetrail/` plumbing outside `records/`), or other.
//!
//! No I/O: the classification is purely path-shape based. Callers compute
//! the path list elsewhere (e.g. `git diff --name-only`) and pass it in.

use std::path::{Component, Path};

use ft_core::RecordKind;

use crate::{RECORDS_DIR, kind_dir};

/// Coarse classification of a single changed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeClass {
    /// File under `.firetrail/records/<memory-kind>/` for a memory kind
    /// (Incident, Finding, Runbook, Decision, Gotcha, Memory).
    Memory(RecordKind),
    /// File under `.firetrail/records/<structural-kind>/` for a structural
    /// kind (Task, Epic, Subtask, Bug).
    Structural(RecordKind),
    /// File under `.firetrail/` but not under `records/` (config,
    /// scope files, lock files, indices, etc.).
    Config,
    /// Anything else — code, docs, CI, vendored data, …
    Other,
}

impl ChangeClass {
    /// `true` iff this classification represents a memory-kind record file.
    #[must_use]
    pub fn is_memory(&self) -> bool {
        matches!(self, Self::Memory(_))
    }
}

/// Return `true` iff `kind` is one of the memory kinds defined in ADR-0009.
#[must_use]
pub fn is_memory_kind(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::Incident
            | RecordKind::Finding
            | RecordKind::Runbook
            | RecordKind::Decision
            | RecordKind::Gotcha
            | RecordKind::Memory
    )
}

/// Every record kind, used to scan for a matching subdirectory.
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
    RecordKind::Doc,
];

/// Classify a single changed path.
///
/// The classification is purely lexical: a path that begins with
/// `.firetrail/records/<kind>/` is treated as a record file for that kind
/// — even if the file does not currently exist on disk (e.g. a deletion
/// in a diff). Trailing path components beyond the kind directory are
/// not validated.
///
/// `path` may be relative to the repo root or absolute. Absolute paths
/// match if they contain `.firetrail/records/<kind>` as a normalized
/// suffix.
#[must_use]
pub fn classify_change(path: &Path) -> ChangeClass {
    // Walk components, looking for the `.firetrail` / `records` /
    // `<kind>` triple in order.
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    // Find the `.firetrail` component.
    let Some(ft_idx) = comps.iter().position(|c| *c == ".firetrail") else {
        return ChangeClass::Other;
    };

    // Anything immediately under `.firetrail/` that isn't `records/` is
    // Config — e.g. `.firetrail/scope.yaml`, `.firetrail/index.sqlite`,
    // `.firetrail/identities.yaml`.
    let after_ft = &comps[ft_idx + 1..];
    // Anything under `.firetrail/` that isn't `records/` (or a bare
    // `.firetrail`) is Config.
    if !matches!(after_ft.first().copied(), Some("records")) {
        return ChangeClass::Config;
    }

    // Need at least `.firetrail/records/<kind>/<file>`. A path that ends
    // at `<kind>/` (no leaf file) is still classified by its kind dir
    // because the caller's intent is clear (e.g. directory-level add).
    let Some(kind_name) = after_ft.get(1).copied() else {
        return ChangeClass::Config;
    };

    // Map directory name → RecordKind via the canonical `kind_dir` table.
    let Some(kind) = ALL_KINDS
        .iter()
        .copied()
        .find(|k| kind_dir(*k) == kind_name)
    else {
        // `.firetrail/records/<unknown>/...` — treat as Other so it
        // surfaces as suspicious. We deliberately do not call it Config.
        return ChangeClass::Other;
    };

    if is_memory_kind(kind) {
        ChangeClass::Memory(kind)
    } else {
        ChangeClass::Structural(kind)
    }
}

/// Return `true` iff every path in `changed_paths` is a memory-kind record
/// file (per [`classify_change`]). An empty slice returns `false` — a
/// commit that touches zero files is not a "memory-only" commit, it's a
/// no-op.
///
/// This is the gate CI hooks use to enforce the relaxed memory-only PR
/// policy from ADR-0009.
#[must_use]
pub fn is_memory_only_change<P: AsRef<Path>>(changed_paths: &[P]) -> bool {
    if changed_paths.is_empty() {
        return false;
    }
    changed_paths
        .iter()
        .all(|p| classify_change(p.as_ref()).is_memory())
}

/// Convenience: full path under `RECORDS_DIR` for a kind directory.
/// Exposed mostly for tests and CLI helpers.
#[must_use]
pub fn records_kind_subpath(kind: RecordKind) -> std::path::PathBuf {
    std::path::PathBuf::from(RECORDS_DIR).join(kind_dir(kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classify_every_memory_kind() {
        for k in [
            RecordKind::Incident,
            RecordKind::Finding,
            RecordKind::Runbook,
            RecordKind::Decision,
            RecordKind::Gotcha,
            RecordKind::Memory,
        ] {
            let p = PathBuf::from(RECORDS_DIR)
                .join(kind_dir(k))
                .join("abc.json");
            assert_eq!(classify_change(&p), ChangeClass::Memory(k), "kind {k:?}");
        }
    }

    #[test]
    fn classify_every_structural_kind() {
        for k in [
            RecordKind::Task,
            RecordKind::Epic,
            RecordKind::Subtask,
            RecordKind::Bug,
        ] {
            let p = PathBuf::from(RECORDS_DIR)
                .join(kind_dir(k))
                .join("abc.json");
            assert_eq!(
                classify_change(&p),
                ChangeClass::Structural(k),
                "kind {k:?}"
            );
        }
    }

    #[test]
    fn classify_config_paths() {
        for p in [
            ".firetrail/scope.yaml",
            ".firetrail/identities.yaml",
            ".firetrail/index.sqlite",
            ".firetrail/lock",
        ] {
            assert_eq!(classify_change(Path::new(p)), ChangeClass::Config, "{p}");
        }
    }

    #[test]
    fn classify_other_paths() {
        for p in ["src/lib.rs", "README.md", "docs/foo.md", "Cargo.toml"] {
            assert_eq!(classify_change(Path::new(p)), ChangeClass::Other, "{p}");
        }
    }

    #[test]
    fn classify_unknown_kind_dir_is_other() {
        let p = Path::new(".firetrail/records/unknown_kind/x.json");
        assert_eq!(classify_change(p), ChangeClass::Other);
    }

    #[test]
    fn classify_absolute_path_works() {
        let p = Path::new("/abs/repo/.firetrail/records/memory/x.json");
        assert_eq!(classify_change(p), ChangeClass::Memory(RecordKind::Memory));
    }

    #[test]
    fn is_memory_only_empty_returns_false() {
        let v: Vec<PathBuf> = Vec::new();
        assert!(!is_memory_only_change(&v));
    }

    #[test]
    fn is_memory_only_all_memory_true() {
        let paths = vec![
            PathBuf::from(".firetrail/records/memory/a.json"),
            PathBuf::from(".firetrail/records/incident/b.json"),
            PathBuf::from(".firetrail/records/runbook/c.json"),
        ];
        assert!(is_memory_only_change(&paths));
    }

    #[test]
    fn is_memory_only_mixed_false() {
        let paths = vec![
            PathBuf::from(".firetrail/records/memory/a.json"),
            PathBuf::from(".firetrail/records/task/b.json"),
        ];
        assert!(!is_memory_only_change(&paths));
    }

    #[test]
    fn is_memory_only_with_other_false() {
        let paths = vec![
            PathBuf::from(".firetrail/records/memory/a.json"),
            PathBuf::from("src/main.rs"),
        ];
        assert!(!is_memory_only_change(&paths));
    }

    #[test]
    fn is_memory_only_with_config_false() {
        let paths = vec![
            PathBuf::from(".firetrail/records/memory/a.json"),
            PathBuf::from(".firetrail/scope.yaml"),
        ];
        assert!(!is_memory_only_change(&paths));
    }
}
