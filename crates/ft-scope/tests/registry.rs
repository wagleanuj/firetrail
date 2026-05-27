//! Unit and integration tests for [`ft_scope::ScopeRegistry`].

use std::path::Path;

use ft_core::Decision;
use ft_core::{Identity, Priority, RecordBody, RecordBuilder, RecordKind, Status};
use ft_scope::{ScopeRegistry, detect_conflicting_decisions};
use tempfile::TempDir;

const SAMPLE_YAML: &str = r"
scopes:
  - id: apps/checkout
    name: Checkout
    applies_to:
      - apps/checkout/**
      - libs/payment-types/**
    aliases: [checkout, ckout]
    codeowners: apps/checkout/CODEOWNERS
  - id: apps/payment
    name: Payment
    applies_to:
      - apps/payment/**
    aliases: [payment]
enabled_scopes:
  - apps/checkout
";

fn write_sample(root: &Path) {
    let dotdir = root.join(".firetrail");
    std::fs::create_dir_all(&dotdir).unwrap();
    std::fs::write(dotdir.join("scopes.yaml"), SAMPLE_YAML).unwrap();

    let checkout_owners = root.join("apps/checkout");
    std::fs::create_dir_all(&checkout_owners).unwrap();
    std::fs::write(
        checkout_owners.join("CODEOWNERS"),
        "# checkout team\napps/checkout/**.rs @alice @bob\napps/checkout/billing/** @billing-team   # billing subteam\n\n",
    )
    .unwrap();
}

#[test]
fn loads_real_scopes_yaml_from_tempdir() {
    let tmp = TempDir::new().unwrap();
    write_sample(tmp.path());

    let reg = ScopeRegistry::load(tmp.path()).expect("load registry");
    assert_eq!(reg.scopes().len(), 2);
    assert_eq!(reg.scopes()[0].id, "apps/checkout");
    assert_eq!(reg.scopes()[0].name, "Checkout");
    assert_eq!(reg.scopes()[0].aliases, vec!["checkout", "ckout"]);
    let owners = reg.scopes()[0].codeowners.as_ref().unwrap();
    assert_eq!(owners.len(), 2);
}

#[test]
fn missing_scopes_file_yields_empty_registry() {
    let tmp = TempDir::new().unwrap();
    let reg = ScopeRegistry::load(tmp.path()).unwrap();
    assert!(reg.is_empty());
}

#[test]
fn scopes_for_path_matches_via_glob() {
    let tmp = TempDir::new().unwrap();
    write_sample(tmp.path());
    let reg = ScopeRegistry::load(tmp.path()).unwrap();

    let matches = reg.scopes_for_path(Path::new("apps/checkout/src/main.rs"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "apps/checkout");

    let multi = reg.scopes_for_path(Path::new("libs/payment-types/src/lib.rs"));
    assert_eq!(multi.len(), 1);
    assert_eq!(multi[0].id, "apps/checkout");

    let none = reg.scopes_for_path(Path::new("apps/unrelated/src/lib.rs"));
    assert!(none.is_empty());
}

#[test]
fn matching_is_case_sensitive() {
    let tmp = TempDir::new().unwrap();
    write_sample(tmp.path());
    let reg = ScopeRegistry::load(tmp.path()).unwrap();

    // Uppercase variant of the path must not match the lowercase glob.
    let upper = reg.scopes_for_path(Path::new("Apps/Checkout/src/main.rs"));
    assert!(upper.is_empty());
}

#[test]
fn alias_resolves() {
    let tmp = TempDir::new().unwrap();
    write_sample(tmp.path());
    let reg = ScopeRegistry::load(tmp.path()).unwrap();

    let by_alias = reg.resolve_alias("checkout").unwrap();
    assert_eq!(by_alias.id, "apps/checkout");
    let short = reg.resolve_alias("ckout").unwrap();
    assert_eq!(short.id, "apps/checkout");
    let by_id = reg.resolve_alias("apps/checkout").unwrap();
    assert_eq!(by_id.id, "apps/checkout");
    assert!(reg.resolve_alias("nonexistent").is_none());
}

#[test]
fn enabled_scopes_filters_pilot() {
    let tmp = TempDir::new().unwrap();
    write_sample(tmp.path());
    let reg = ScopeRegistry::load(tmp.path()).unwrap();

    assert!(reg.is_scope_enabled("apps/checkout"));
    assert!(!reg.is_scope_enabled("apps/payment"));
    assert!(!reg.is_scope_enabled("apps/never-declared"));
}

#[test]
fn enabled_scopes_none_means_all_enabled() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".firetrail")).unwrap();
    std::fs::write(
        tmp.path().join(".firetrail/scopes.yaml"),
        "scopes:\n  - id: apps/checkout\n    applies_to:\n      - apps/checkout/**\n",
    )
    .unwrap();
    let reg = ScopeRegistry::load(tmp.path()).unwrap();
    assert!(reg.is_scope_enabled("apps/checkout"));
    assert!(reg.is_scope_enabled("anything-else"));
}

#[test]
fn owners_for_path_resolves_codeowners() {
    let tmp = TempDir::new().unwrap();
    write_sample(tmp.path());
    let reg = ScopeRegistry::load(tmp.path()).unwrap();

    let owners = reg.owners_for_path(Path::new("apps/checkout/src/main.rs"));
    let strs: Vec<&str> = owners.iter().map(Identity::as_str).collect();
    assert_eq!(strs, vec!["@alice", "@bob"]);

    let billing = reg.owners_for_path(Path::new("apps/checkout/billing/invoice.rs"));
    // The .rs pattern matches as well, so we expect alice/bob first, then
    // the billing-team owner from the second rule. Deduped & in-order.
    let strs: Vec<&str> = billing.iter().map(Identity::as_str).collect();
    assert_eq!(strs, vec!["@alice", "@bob", "@billing-team"]);

    let outside = reg.owners_for_path(Path::new("apps/unrelated/x.rs"));
    assert!(outside.is_empty());
}

#[test]
fn duplicate_scope_id_errors() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".firetrail")).unwrap();
    std::fs::write(
        tmp.path().join(".firetrail/scopes.yaml"),
        "scopes:\n  - id: a\n    applies_to: [a/**]\n  - id: a\n    applies_to: [b/**]\n",
    )
    .unwrap();
    let err = ScopeRegistry::load(tmp.path()).unwrap_err();
    assert!(matches!(err, ft_scope::ScopeError::DuplicateScopeId { .. }));
}

#[test]
fn duplicate_alias_errors() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".firetrail")).unwrap();
    std::fs::write(
        tmp.path().join(".firetrail/scopes.yaml"),
        "scopes:\n  - id: a\n    aliases: [shared]\n    applies_to: [a/**]\n  - id: b\n    aliases: [shared]\n    applies_to: [b/**]\n",
    )
    .unwrap();
    let err = ScopeRegistry::load(tmp.path()).unwrap_err();
    assert!(matches!(err, ft_scope::ScopeError::DuplicateAlias { .. }));
}

fn make_decision_record(
    id_seed: &str,
    title: &str,
    owning_scope: Option<&str>,
    body: &str,
) -> ft_core::Record {
    let alice = Identity::new("alice@example.com").unwrap();
    let mut builder = RecordBuilder::new(RecordKind::Decision, title, alice)
        .status(Status::Open)
        .priority(Priority::P2)
        .body(RecordBody::Decision(Decision {
            title: title.to_string(),
            context: format!("context for {id_seed}"),
            decision: body.to_string(),
            consequences: String::new(),
            alternatives_considered: vec![],
            status: ft_core::DecisionStatus::default(),
            superseded_by: None,
            risk_class: None,
            trust: ft_core::TrustState::Draft,
        }));
    if let Some(s) = owning_scope {
        builder = builder.owning_scope(s);
    }
    builder.build().expect("build decision")
}

#[test]
fn detect_conflicting_decisions_finds_divergent_same_id_across_scopes() {
    let a = make_decision_record(
        "a",
        "ADR-0100: shared cache eviction",
        Some("apps/checkout"),
        "evict on TTL only",
    );
    let b = make_decision_record(
        "b",
        "ADR-0100: shared cache eviction",
        Some("apps/payment"),
        "evict on TTL and on user logout",
    );
    let c = make_decision_record(
        "c",
        "ADR-0200: unrelated",
        Some("apps/checkout"),
        "do thing",
    );

    let conflicts = detect_conflicting_decisions(&[a, b, c]);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].external_id, "adr-0100: shared cache eviction");
    assert_eq!(conflicts[0].occurrences.len(), 2);
}

#[test]
fn detect_conflicting_decisions_ignores_identical_bodies() {
    let body = "evict on TTL only";
    let a = make_decision_record("a", "ADR-X", Some("apps/checkout"), body);
    let b = make_decision_record("b", "ADR-X", Some("apps/payment"), body);
    let conflicts = detect_conflicting_decisions(&[a, b]);
    assert!(conflicts.is_empty());
}

#[test]
fn detect_conflicting_decisions_ignores_same_scope_duplicates() {
    let a = make_decision_record("a", "ADR-Y", Some("apps/checkout"), "body 1");
    let b = make_decision_record("b", "ADR-Y", Some("apps/checkout"), "body 2");
    let conflicts = detect_conflicting_decisions(&[a, b]);
    assert!(conflicts.is_empty());
}
