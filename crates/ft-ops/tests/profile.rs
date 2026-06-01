//! Integration tests for `ft_ops::profile` — the embedded repo-profile surface
//! the ft-ui Profile panel calls (`RepoProfile` epic).
//!
//! Mirrors `tests/docs.rs`: an isolated `TestRepo` with `.firetrail/config.yml`,
//! ops exercised directly (no CLI shell-out).

use ft_ops::profile::{self, AddComponentInput, UpdateProfileInput};
use ft_ops::{EventBus, Identity, OpsError, Workspace};
use ft_testkit::TestRepo;

fn write_scopes(tr: &TestRepo) {
    let firetrail = tr.firetrail_dir();
    std::fs::write(
        firetrail.join("scopes.yaml"),
        "scopes:\n  - id: apps/checkout\n    applies_to: [\"apps/checkout/**\"]\n",
    )
    .expect("write scopes.yaml");
}

fn fixture() -> (TestRepo, Workspace) {
    let tr = TestRepo::new().expect("test repo");
    let firetrail = tr.firetrail_dir();
    std::fs::create_dir_all(&firetrail).expect("mkdir .firetrail");
    std::fs::write(
        firetrail.join("config.yml"),
        "schema_version: 1\nidentity:\n  strict: false\n",
    )
    .expect("write config.yml");
    let ws = Workspace::open(tr.root()).expect("open workspace");
    (tr, ws)
}

fn alice() -> Identity {
    Identity::new("alice@firetrail.test", "Alice")
}

fn bus() -> EventBus {
    EventBus::new(64)
}

#[test]
fn get_returns_none_when_absent() {
    let (_tr, ws) = fixture();
    let out = profile::get(&ws, &alice(), &bus()).expect("get");
    assert!(out.is_none(), "no profile yet");
}

#[test]
fn update_creates_when_absent() {
    let (_tr, ws) = fixture();
    let input = UpdateProfileInput {
        validate_command: Some(Some("cargo test".into())),
        languages: Some(vec!["rust".into()]),
        ..Default::default()
    };
    let view = profile::update(&ws, &alice(), input, &bus()).expect("update");
    assert_eq!(view.validate_command.as_deref(), Some("cargo test"));
    assert_eq!(view.languages, vec!["rust".to_string()]);
    // New body stays Draft — trust is not auto-transitioned.
    assert_eq!(view.trust, "draft");

    // Persisted: a follow-up get returns the same record.
    let back = profile::get(&ws, &alice(), &bus())
        .expect("get")
        .expect("present");
    assert_eq!(back.id, view.id);
    assert_eq!(back.validate_command.as_deref(), Some("cargo test"));
}

#[test]
fn update_partial_preserves_untouched_fields() {
    let (_tr, ws) = fixture();
    // Seed validate + test commands.
    profile::update(
        &ws,
        &alice(),
        UpdateProfileInput {
            validate_command: Some(Some("just ci".into())),
            test_command: Some(Some("cargo test".into())),
            languages: Some(vec!["rust".into()]),
            ..Default::default()
        },
        &bus(),
    )
    .expect("seed");

    // Update only the build command — everything else is preserved.
    let view = profile::update(
        &ws,
        &alice(),
        UpdateProfileInput {
            build_command: Some(Some("cargo build".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("update");

    assert_eq!(view.build_command.as_deref(), Some("cargo build"));
    assert_eq!(view.validate_command.as_deref(), Some("just ci"));
    assert_eq!(view.test_command.as_deref(), Some("cargo test"));
    assert_eq!(view.languages, vec!["rust".to_string()]);

    // It's a singleton update, not a duplicate.
    let seeded_id = profile::get(&ws, &alice(), &bus()).unwrap().unwrap().id;
    assert_eq!(view.id, seeded_id);
}

#[test]
fn update_can_clear_a_field_with_explicit_none() {
    let (_tr, ws) = fixture();
    profile::update(
        &ws,
        &alice(),
        UpdateProfileInput {
            validate_command: Some(Some("cargo test".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("seed");
    let view = profile::update(
        &ws,
        &alice(),
        UpdateProfileInput {
            validate_command: Some(None),
            ..Default::default()
        },
        &bus(),
    )
    .expect("clear");
    assert_eq!(view.validate_command, None);
}

#[test]
fn add_and_remove_component_round_trip() {
    let (_tr, ws) = fixture();
    // add_component creates the profile if absent.
    let view = profile::add_component(
        &ws,
        &alice(),
        AddComponentInput {
            name: "ft-ops".into(),
            path: "crates/ft-ops".into(),
            summary: Some("ops layer".into()),
            request_id: None,
        },
        &bus(),
    )
    .expect("add");
    assert_eq!(view.components.len(), 1);
    assert_eq!(view.components[0].name, "ft-ops");
    assert_eq!(view.components[0].path, "crates/ft-ops");

    // Adding a second.
    let view = profile::add_component(
        &ws,
        &alice(),
        AddComponentInput {
            name: "ft-ui".into(),
            path: "crates/ft-ui".into(),
            summary: None,
            request_id: None,
        },
        &bus(),
    )
    .expect("add 2");
    assert_eq!(view.components.len(), 2);

    // Adding by an existing name replaces in place (no duplicate).
    let view = profile::add_component(
        &ws,
        &alice(),
        AddComponentInput {
            name: "ft-ops".into(),
            path: "crates/ft-ops-new".into(),
            summary: None,
            request_id: None,
        },
        &bus(),
    )
    .expect("replace");
    assert_eq!(view.components.len(), 2);
    let ops = view.components.iter().find(|c| c.name == "ft-ops").unwrap();
    assert_eq!(ops.path, "crates/ft-ops-new");

    // Remove one.
    let view = profile::remove_component(&ws, &alice(), "ft-ops".into(), &bus()).expect("remove");
    assert_eq!(view.components.len(), 1);
    assert_eq!(view.components[0].name, "ft-ui");
}

#[test]
fn remove_missing_component_is_not_found() {
    let (_tr, ws) = fixture();
    profile::add_component(
        &ws,
        &alice(),
        AddComponentInput {
            name: "ft-ops".into(),
            path: "crates/ft-ops".into(),
            summary: None,
            request_id: None,
        },
        &bus(),
    )
    .expect("add");
    let err = profile::remove_component(&ws, &alice(), "does-not-exist".into(), &bus())
        .expect_err("should 404");
    assert!(matches!(err, OpsError::NotFound { .. }), "got {err:?}");
}

#[test]
fn remove_from_absent_profile_is_not_found() {
    let (_tr, ws) = fixture();
    let err = profile::remove_component(&ws, &alice(), "anything".into(), &bus())
        .expect_err("should 404");
    assert!(matches!(err, OpsError::NotFound { .. }), "got {err:?}");
}

// ── Scope-aware variants (Phase 5.1) ─────────────────────────────────────────

#[test]
fn get_for_scope_returns_none_when_absent() {
    let (_tr, ws) = fixture();
    let out = profile::get_for_scope(&ws, &alice(), "apps/checkout", false, &bus()).expect("get");
    assert!(out.is_none(), "no scope delta yet");
}

#[test]
fn update_for_scope_writes_a_scoped_record() {
    let (_tr, ws) = fixture();
    let view = profile::update_for_scope(
        &ws,
        &alice(),
        "apps/checkout",
        UpdateProfileInput {
            test_command: Some(Some("pnpm test".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("update scope");
    assert_eq!(view.test_command.as_deref(), Some("pnpm test"));

    // The raw delta is readable back; it does not leak into the base.
    let delta = profile::get_for_scope(&ws, &alice(), "apps/checkout", false, &bus())
        .expect("get")
        .expect("present");
    assert_eq!(delta.id, view.id);
    assert_eq!(delta.test_command.as_deref(), Some("pnpm test"));
    // No base record was created — resolving against the (absent) base leaves
    // the scope's fields exactly as stored (nothing inherited).
    let resolved = profile::get_for_scope(&ws, &alice(), "apps/checkout", true, &bus())
        .expect("resolved get")
        .expect("present");
    assert_eq!(resolved.test_command.as_deref(), Some("pnpm test"));
    assert_eq!(resolved.validate_command, None, "no base to inherit from");
}

#[test]
fn get_for_scope_resolved_merges_base_and_delta() {
    let (_tr, ws) = fixture();
    // Base supplies validate; scope delta overrides test.
    profile::update(
        &ws,
        &alice(),
        UpdateProfileInput {
            validate_command: Some(Some("just ci".into())),
            test_command: Some(Some("cargo test".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("seed base");
    profile::update_for_scope(
        &ws,
        &alice(),
        "apps/checkout",
        UpdateProfileInput {
            test_command: Some(Some("pnpm test".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("seed scope");

    // resolved=false → raw delta (validate inherited only when merged).
    let raw = profile::get_for_scope(&ws, &alice(), "apps/checkout", false, &bus())
        .expect("get raw")
        .expect("present");
    assert_eq!(raw.test_command.as_deref(), Some("pnpm test"));
    assert_eq!(raw.validate_command, None, "raw delta does not carry base");

    // resolved=true → merge(base, delta): validate inherited, test overridden.
    let resolved = profile::get_for_scope(&ws, &alice(), "apps/checkout", true, &bus())
        .expect("get resolved")
        .expect("present");
    assert_eq!(resolved.validate_command.as_deref(), Some("just ci"));
    assert_eq!(resolved.test_command.as_deref(), Some("pnpm test"));
}

#[test]
fn validate_plan_op_resolves_changeset() {
    let (tr, ws) = fixture();
    write_scopes(&tr);
    profile::update(
        &ws,
        &alice(),
        UpdateProfileInput {
            validate_command: Some(Some("just ci".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("seed base");
    profile::update_for_scope(
        &ws,
        &alice(),
        "apps/checkout",
        UpdateProfileInput {
            validate_command: Some(Some("pnpm --filter checkout test".into())),
            ..Default::default()
        },
        &bus(),
    )
    .expect("seed scope");

    let plan = profile::validate_plan(
        &ws,
        &alice(),
        &[
            "apps/checkout/a.ts".into(),
            "apps/checkout/b.ts".into(),
            "README.md".into(),
        ],
        &bus(),
    )
    .expect("plan");
    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.unresolved, 0);
    let checkout = plan
        .entries
        .iter()
        .find(|e| e.command.contains("checkout"))
        .expect("checkout entry");
    assert_eq!(checkout.file_count, 2);
    assert_eq!(checkout.scopes, vec!["apps/checkout".to_string()]);
}
