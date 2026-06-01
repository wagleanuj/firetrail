//! Tests for the `.firetrail/scopes.yaml` write path ([`ft_scope::writer`]).

use std::path::Path;

use ft_scope::{
    HEADER, ScopeError, ScopeRegistry, ScopeYaml, ScopesFile, load_file, remove_scope, reorder,
    save_file, upsert_scope, validate,
};
use ft_testkit::TestRepo;

/// Build a scope with a single trivial `applies_to` pattern.
fn scope(id: &str) -> ScopeYaml {
    ScopeYaml {
        id: id.to_string(),
        applies_to: vec![format!("{id}/**")],
        ..Default::default()
    }
}

fn ids_in_order(file: &ScopesFile) -> Vec<String> {
    file.scopes.iter().map(|s| s.id.clone()).collect()
}

fn loaded_ids(root: &Path) -> Vec<String> {
    let reg = ScopeRegistry::load(root).unwrap();
    reg.scopes().iter().map(|s| s.id.clone()).collect()
}

#[test]
fn save_then_load_round_trips_in_order() {
    let repo = TestRepo::new().unwrap();
    let file = ScopesFile {
        scopes: vec![scope("apps/a"), scope("apps/b"), scope("apps/c")],
        ..Default::default()
    };

    save_file(repo.root(), &file).unwrap();

    // ScopeRegistry::load sees the same scopes, in declaration order.
    let reg = ScopeRegistry::load(repo.root()).unwrap();
    let ids: Vec<String> = reg.scopes().iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, vec!["apps/a", "apps/b", "apps/c"]);

    // Globs survived the round-trip.
    assert!(
        reg.get("apps/b")
            .unwrap()
            .matches_path(Path::new("apps/b/main.rs"))
    );
}

#[test]
fn written_file_has_header_comment() {
    let repo = TestRepo::new().unwrap();
    let file = ScopesFile {
        scopes: vec![scope("apps/a")],
        ..Default::default()
    };
    save_file(repo.root(), &file).unwrap();

    let text = std::fs::read_to_string(repo.root().join(".firetrail/scopes.yaml")).unwrap();
    assert!(
        text.starts_with(HEADER),
        "file should start with the managed header, got:\n{text}"
    );
}

#[test]
fn upsert_new_id_appends_last() {
    let mut file = ScopesFile {
        scopes: vec![scope("apps/a"), scope("apps/b")],
        ..Default::default()
    };
    upsert_scope(&mut file, scope("apps/c")).unwrap();
    assert_eq!(ids_in_order(&file), vec!["apps/a", "apps/b", "apps/c"]);
}

#[test]
fn upsert_existing_id_replaces_in_place() {
    let repo = TestRepo::new().unwrap();
    let mut file = ScopesFile {
        scopes: vec![scope("apps/a"), scope("apps/b"), scope("apps/c")],
        ..Default::default()
    };

    let replacement = ScopeYaml {
        id: "apps/b".to_string(),
        name: Some("Beta".to_string()),
        applies_to: vec!["apps/beta/**".to_string()],
        ..Default::default()
    };
    upsert_scope(&mut file, replacement).unwrap();

    // Position preserved.
    assert_eq!(ids_in_order(&file), vec!["apps/a", "apps/b", "apps/c"]);
    // Content replaced.
    assert_eq!(file.scopes[1].name.as_deref(), Some("Beta"));

    save_file(repo.root(), &file).unwrap();
    let reg = ScopeRegistry::load(repo.root()).unwrap();
    assert_eq!(reg.get("apps/b").unwrap().name, "Beta");
    assert!(
        reg.get("apps/b")
            .unwrap()
            .matches_path(Path::new("apps/beta/x.rs"))
    );
    assert_eq!(loaded_ids(repo.root()), vec!["apps/a", "apps/b", "apps/c"]);
}

#[test]
fn remove_scope_removes_present_and_errors_on_absent() {
    let mut file = ScopesFile {
        scopes: vec![scope("apps/a"), scope("apps/b")],
        ..Default::default()
    };
    remove_scope(&mut file, "apps/a").unwrap();
    assert_eq!(ids_in_order(&file), vec!["apps/b"]);

    let err = remove_scope(&mut file, "apps/missing").unwrap_err();
    assert!(matches!(err, ScopeError::ScopeNotFound { id } if id == "apps/missing"));
}

#[test]
fn reorder_permutation_reorders_and_non_permutation_errors() {
    let mut file = ScopesFile {
        scopes: vec![scope("apps/a"), scope("apps/b"), scope("apps/c")],
        ..Default::default()
    };

    reorder(
        &mut file,
        &[
            "apps/c".to_string(),
            "apps/a".to_string(),
            "apps/b".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(ids_in_order(&file), vec!["apps/c", "apps/a", "apps/b"]);

    // Wrong length.
    let err = reorder(&mut file, &["apps/c".to_string()]).unwrap_err();
    assert!(matches!(err, ScopeError::ReorderMismatch));
    // Order unchanged after a failed reorder.
    assert_eq!(ids_in_order(&file), vec!["apps/c", "apps/a", "apps/b"]);

    // Right length but unknown id (and a missing one).
    let err = reorder(
        &mut file,
        &[
            "apps/c".to_string(),
            "apps/a".to_string(),
            "apps/ghost".to_string(),
        ],
    )
    .unwrap_err();
    assert!(matches!(err, ScopeError::ReorderMismatch));
    assert_eq!(ids_in_order(&file), vec!["apps/c", "apps/a", "apps/b"]);

    // Right length but a duplicate id.
    let err = reorder(
        &mut file,
        &[
            "apps/c".to_string(),
            "apps/c".to_string(),
            "apps/a".to_string(),
        ],
    )
    .unwrap_err();
    assert!(matches!(err, ScopeError::ReorderMismatch));
    assert_eq!(ids_in_order(&file), vec!["apps/c", "apps/a", "apps/b"]);
}

#[test]
fn validate_rejects_invalid_glob() {
    let file = ScopesFile {
        scopes: vec![ScopeYaml {
            id: "apps/a".to_string(),
            applies_to: vec!["a/[".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let err = validate(&file).unwrap_err();
    assert!(matches!(err, ScopeError::InvalidGlob { scope_id, .. } if scope_id == "apps/a"));
}

#[test]
fn validate_rejects_duplicate_id() {
    let file = ScopesFile {
        scopes: vec![scope("apps/a"), scope("apps/a")],
        ..Default::default()
    };
    let err = validate(&file).unwrap_err();
    assert!(matches!(err, ScopeError::DuplicateScopeId { id } if id == "apps/a"));
}

#[test]
fn validate_rejects_duplicate_alias() {
    let file = ScopesFile {
        scopes: vec![
            ScopeYaml {
                id: "apps/a".to_string(),
                applies_to: vec!["apps/a/**".to_string()],
                aliases: vec!["shared".to_string()],
                ..Default::default()
            },
            ScopeYaml {
                id: "apps/b".to_string(),
                applies_to: vec!["apps/b/**".to_string()],
                aliases: vec!["shared".to_string()],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let err = validate(&file).unwrap_err();
    assert!(
        matches!(err, ScopeError::DuplicateAlias { alias, first, second }
            if alias == "shared" && first == "apps/a" && second == "apps/b")
    );
}

#[test]
fn validate_rejects_empty_applies_to() {
    let file = ScopesFile {
        scopes: vec![ScopeYaml {
            id: "apps/a".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let err = validate(&file).unwrap_err();
    assert!(matches!(err, ScopeError::EmptyAppliesTo { id } if id == "apps/a"));
}

#[test]
fn save_rejects_invalid_model() {
    let repo = TestRepo::new().unwrap();
    let file = ScopesFile {
        scopes: vec![ScopeYaml {
            id: "apps/a".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    // save_file validates first, so nothing is written.
    let err = save_file(repo.root(), &file).unwrap_err();
    assert!(matches!(err, ScopeError::EmptyAppliesTo { .. }));
    assert!(!repo.root().join(".firetrail/scopes.yaml").exists());
}

#[test]
fn absent_file_loads_empty_and_save_creates_it() {
    let repo = TestRepo::new().unwrap();
    // TestRepo creates .firetrail/ but no scopes.yaml.
    assert!(!repo.root().join(".firetrail/scopes.yaml").exists());

    let file = load_file(repo.root()).unwrap();
    assert!(file.scopes.is_empty());
    assert!(file.enabled_scopes.is_none());

    let mut file = file;
    upsert_scope(&mut file, scope("apps/a")).unwrap();
    save_file(repo.root(), &file).unwrap();

    assert!(repo.root().join(".firetrail/scopes.yaml").exists());
    assert_eq!(loaded_ids(repo.root()), vec!["apps/a"]);
}

#[test]
fn save_creates_firetrail_dir_when_missing() {
    // A bare temp dir with no .firetrail/ at all.
    let dir = tempfile::tempdir().unwrap();
    let file = ScopesFile {
        scopes: vec![scope("apps/a")],
        ..Default::default()
    };
    save_file(dir.path(), &file).unwrap();
    assert!(dir.path().join(".firetrail/scopes.yaml").exists());
}
