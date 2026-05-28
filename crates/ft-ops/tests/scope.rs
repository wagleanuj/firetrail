//! Integration tests for `ft_ops::scope`.

use std::path::PathBuf;

use ft_ops::scope::{self, AliasesInput, ListInput, OwnersInput, ShowInput};
use ft_ops::{EventBus, Identity, Workspace};
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
