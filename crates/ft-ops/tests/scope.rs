//! Integration tests for `ft_ops::scope`.

use std::path::PathBuf;

use ft_ops::scope::{
    self, AliasesInput, ListInput, OwnersInput, ScopeEditInput, ScopeInput, ShowInput,
};
use ft_ops::{EventBus, Identity, Workspace};
use ft_scope::ScopeRegistry;
use ft_testkit::TestRepo;

fn fixture_with_scopes(scopes_yaml: &str) -> (TestRepo, Workspace) {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).unwrap();
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .unwrap();
    std::fs::write(firetrail.join("scopes.yaml"), scopes_yaml).unwrap();
    let ws = Workspace::open(tr.root()).unwrap();
    (tr, ws)
}

fn alice() -> Identity {
    Identity::new("alice@firetrail.test", "Alice")
}

#[test]
fn list_returns_loaded_scopes() {
    let (_tr, ws) = fixture_with_scopes(
        "scopes:\n  - id: apps/checkout\n    name: Checkout\n    applies_to:\n      - apps/checkout/**\n    aliases: [checkout, co]\n",
    );
    let out = scope::list(&ws, &alice(), ListInput::default(), &EventBus::default()).unwrap();
    assert_eq!(out.scopes.len(), 1);
    let s = &out.scopes[0];
    assert_eq!(s.id, "apps/checkout");
    assert_eq!(s.applies_to, vec!["apps/checkout/**".to_string()]);
    assert!(s.aliases.contains(&"checkout".into()));
}

#[test]
fn show_resolves_by_alias_or_id() {
    let (_tr, ws) = fixture_with_scopes(
        "scopes:\n  - id: apps/checkout\n    name: Checkout\n    applies_to: [apps/checkout/**]\n    aliases: [co]\n",
    );
    let out_a = scope::show(
        &ws,
        &alice(),
        ShowInput {
            id: "apps/checkout".into(),
            request_id: None,
        },
        &EventBus::default(),
    )
    .unwrap();
    assert_eq!(out_a.scope.summary.id, "apps/checkout");

    let out_b = scope::show(
        &ws,
        &alice(),
        ShowInput {
            id: "co".into(),
            request_id: None,
        },
        &EventBus::default(),
    )
    .unwrap();
    assert_eq!(out_b.scope.summary.id, "apps/checkout");
}

#[test]
fn show_unknown_returns_not_found() {
    let (_tr, ws) = fixture_with_scopes("scopes: []\n");
    let err = scope::show(
        &ws,
        &alice(),
        ShowInput {
            id: "nope".into(),
            request_id: None,
        },
        &EventBus::default(),
    )
    .unwrap_err();
    assert!(matches!(err, ft_ops::OpsError::NotFound { .. }));
}

#[test]
fn aliases_includes_self_alias() {
    let (_tr, ws) = fixture_with_scopes(
        "scopes:\n  - id: api\n    name: API\n    applies_to: [api/**]\n    aliases: [a]\n",
    );
    let out = scope::aliases(&ws, &alice(), AliasesInput::default(), &EventBus::default()).unwrap();
    let names: Vec<&str> = out.aliases.iter().map(|a| a.alias.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"api"));
}

#[test]
fn owners_returns_empty_when_no_codeowners() {
    let (_tr, ws) = fixture_with_scopes("scopes: []\n");
    let out = scope::owners(
        &ws,
        &alice(),
        OwnersInput {
            path: PathBuf::from("some/file.rs"),
            request_id: None,
        },
        &EventBus::default(),
    )
    .unwrap();
    assert!(out.owners.is_empty());
}

// ── Write ops ────────────────────────────────────────────────────────────────

fn empty_workspace() -> (TestRepo, Workspace) {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).unwrap();
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .unwrap();
    let ws = Workspace::open(tr.root()).unwrap();
    (tr, ws)
}

fn scope_input(id: &str, applies_to: &[&str]) -> ScopeInput {
    ScopeInput {
        id: id.into(),
        name: None,
        applies_to: applies_to.iter().map(|s| (*s).to_string()).collect(),
        aliases: Vec::new(),
        codeowners: None,
        request_id: None,
    }
}

#[test]
fn add_writes_a_scope_the_registry_loads() {
    let (tr, ws) = empty_workspace();
    let view = scope::add(
        &ws,
        &alice(),
        scope_input("apps/checkout", &["apps/checkout/**"]),
        &EventBus::default(),
    )
    .expect("add scope");
    assert!(view.scopes.iter().any(|s| s.id == "apps/checkout"));

    // The registry must load the freshly-written file.
    let reg = ScopeRegistry::load(tr.root()).expect("load registry");
    assert!(reg.get("apps/checkout").is_some());
}

#[test]
fn add_duplicate_id_errors() {
    let (_tr, ws) = empty_workspace();
    scope::add(
        &ws,
        &alice(),
        scope_input("api", &["api/**"]),
        &EventBus::default(),
    )
    .expect("first add");
    let err = scope::add(
        &ws,
        &alice(),
        scope_input("api", &["api/**"]),
        &EventBus::default(),
    )
    .unwrap_err();
    assert!(matches!(err, ft_ops::OpsError::Conflict { .. }), "{err:?}");
}

#[test]
fn add_invalid_glob_is_validation_error() {
    let (_tr, ws) = empty_workspace();
    let err = scope::add(
        &ws,
        &alice(),
        scope_input("bad", &["a/[b"]),
        &EventBus::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ft_ops::OpsError::Validation { .. }),
        "{err:?}"
    );
}

#[test]
fn edit_applies_changes_in_place() {
    let (_tr, ws) = empty_workspace();
    scope::add(
        &ws,
        &alice(),
        scope_input("api", &["api/**"]),
        &EventBus::default(),
    )
    .expect("add");
    let view = scope::edit(
        &ws,
        &alice(),
        "api",
        ScopeEditInput {
            name: Some(Some("API service".into())),
            applies_to: Some(vec!["api/**".into(), "svc/**".into()]),
            aliases: Some(vec!["a".into()]),
            codeowners: None,
            request_id: None,
        },
        &EventBus::default(),
    )
    .expect("edit");
    let s = view.scopes.iter().find(|s| s.id == "api").unwrap();
    assert_eq!(s.name.as_deref(), Some("API service"));
    assert_eq!(s.applies_to.len(), 2);
    assert_eq!(s.aliases, vec!["a".to_string()]);
}

#[test]
fn edit_unknown_is_not_found() {
    let (_tr, ws) = empty_workspace();
    let err = scope::edit(
        &ws,
        &alice(),
        "nope",
        ScopeEditInput::default(),
        &EventBus::default(),
    )
    .unwrap_err();
    assert!(matches!(err, ft_ops::OpsError::NotFound { .. }), "{err:?}");
}

#[test]
fn remove_deletes_scope() {
    let (_tr, ws) = empty_workspace();
    scope::add(
        &ws,
        &alice(),
        scope_input("api", &["api/**"]),
        &EventBus::default(),
    )
    .expect("add");
    let view = scope::remove(&ws, &alice(), "api", &EventBus::default()).expect("remove");
    assert!(view.scopes.is_empty());
}

#[test]
fn remove_unknown_is_not_found() {
    let (_tr, ws) = empty_workspace();
    let err = scope::remove(&ws, &alice(), "nope", &EventBus::default()).unwrap_err();
    assert!(matches!(err, ft_ops::OpsError::NotFound { .. }), "{err:?}");
}

#[test]
fn reorder_changes_declaration_order() {
    let (_tr, ws) = empty_workspace();
    scope::add(
        &ws,
        &alice(),
        scope_input("a", &["a/**"]),
        &EventBus::default(),
    )
    .unwrap();
    scope::add(
        &ws,
        &alice(),
        scope_input("b", &["b/**"]),
        &EventBus::default(),
    )
    .unwrap();
    let view = scope::reorder(
        &ws,
        &alice(),
        &["b".into(), "a".into()],
        &EventBus::default(),
    )
    .expect("reorder");
    let ids: Vec<&str> = view.scopes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "a"]);
}

#[test]
fn reorder_mismatch_is_validation() {
    let (_tr, ws) = empty_workspace();
    scope::add(
        &ws,
        &alice(),
        scope_input("a", &["a/**"]),
        &EventBus::default(),
    )
    .unwrap();
    let err = scope::reorder(
        &ws,
        &alice(),
        &["a".into(), "b".into()],
        &EventBus::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ft_ops::OpsError::Validation { .. }),
        "{err:?}"
    );
}

#[test]
fn preview_reports_match_counts_and_zero_match_warning() {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).unwrap();
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .unwrap();
    // Two tracked files under apps/checkout, none under marketing.
    std::fs::create_dir_all(tr.root().join("apps/checkout")).unwrap();
    std::fs::write(tr.root().join("apps/checkout/a.rs"), "// a\n").unwrap();
    std::fs::write(tr.root().join("apps/checkout/b.rs"), "// b\n").unwrap();
    std::fs::write(
        firetrail.join("scopes.yaml"),
        "scopes:\n  - id: checkout\n    applies_to: [apps/checkout/**]\n  - id: marketing\n    applies_to: [marketing/**]\n",
    )
    .unwrap();
    tr.commit_all("seed files").expect("commit");

    let ws = Workspace::open(tr.root()).unwrap();
    let out = scope::preview(&ws, &alice(), &EventBus::default()).expect("preview");

    let checkout = out.scopes.iter().find(|s| s.id == "checkout").unwrap();
    assert_eq!(checkout.match_count, 2);
    let marketing = out.scopes.iter().find(|s| s.id == "marketing").unwrap();
    assert_eq!(marketing.match_count, 0);

    // marketing matches zero tracked files → a zero-match warning.
    assert!(
        out.warnings.iter().any(|w| w.contains("marketing")),
        "warnings: {:?}",
        out.warnings
    );
}
