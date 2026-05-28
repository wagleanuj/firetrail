//! Integration tests for `ft_ops::identity_ops`.

use ft_ops::identity_ops::{
    self, CapabilitiesInput, IdentityKindInput, IdentityStatusFilter, ListInput, OffboardInput,
    RegisterInput, ShowInput,
};
use ft_ops::{EventBus, Identity, Workspace};
use ft_testkit::TestRepo;

fn fixture() -> (TestRepo, Workspace) {
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

fn alice() -> Identity {
    Identity::new("alice@firetrail.test", "Alice")
}

#[test]
fn register_then_list_and_show() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let mut rx = bus.subscribe();

    let out = identity_ops::register(
        &ws,
        &id,
        RegisterInput {
            id: "alice".into(),
            name: "Alice Test".into(),
            emails: vec!["alice@firetrail.test".into()],
            kind: IdentityKindInput::Human,
            machines: vec![],
            capabilities: vec![],
            request_id: None,
        },
        &bus,
    )
    .expect("register");
    assert_eq!(out.identity.id, "alice");
    assert_eq!(out.identity.kind, "human");
    assert_eq!(out.identity.status, "active");

    // event emitted
    let env = rx.try_recv().expect("identity event");
    match env.event {
        ft_ops::Event::IdentityUpdated { identity, .. } => assert_eq!(identity, "alice"),
        other => panic!("expected IdentityUpdated, got {other:?}"),
    }

    let listed = identity_ops::list(&ws, &id, ListInput::default(), &bus).unwrap();
    assert_eq!(listed.identities.len(), 1);

    let shown = identity_ops::show(
        &ws,
        &id,
        ShowInput {
            id: "alice".into(),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert_eq!(shown.identity.id, "alice");
}

#[test]
fn duplicate_register_returns_conflict() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    let inp = RegisterInput {
        id: "alice".into(),
        name: "Alice".into(),
        emails: vec!["alice@example.com".into()],
        kind: IdentityKindInput::Human,
        machines: vec![],
        capabilities: vec![],
        request_id: None,
    };
    identity_ops::register(&ws, &id, inp.clone(), &bus).unwrap();
    let err = identity_ops::register(&ws, &id, inp, &bus).unwrap_err();
    assert!(matches!(err, ft_ops::OpsError::Conflict { .. }));
}

#[test]
fn list_filters_by_status() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    identity_ops::register(
        &ws,
        &id,
        RegisterInput {
            id: "bob".into(),
            name: "Bob".into(),
            emails: vec!["bob@example.com".into()],
            kind: IdentityKindInput::Human,
            machines: vec![],
            capabilities: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    identity_ops::offboard(
        &ws,
        &id,
        OffboardInput {
            id: "bob".into(),
            sweep_claims: false,
            request_id: None,
        },
        &bus,
    )
    .unwrap();

    let active = identity_ops::list(
        &ws,
        &id,
        ListInput {
            status: Some(IdentityStatusFilter::Active),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert!(active.identities.iter().all(|i| i.status == "active"));

    let off = identity_ops::list(
        &ws,
        &id,
        ListInput {
            status: Some(IdentityStatusFilter::Offboarded),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    assert_eq!(off.identities.len(), 1);
    assert_eq!(off.identities[0].id, "bob");
}

#[test]
fn capabilities_surfaces_effective_matrix() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = EventBus::default();
    identity_ops::register(
        &ws,
        &id,
        RegisterInput {
            id: "ci-runner".into(),
            name: "CI".into(),
            emails: vec!["ci@example.com".into()],
            kind: IdentityKindInput::Ci,
            machines: vec![],
            capabilities: vec![],
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let caps = identity_ops::capabilities(
        &ws,
        &id,
        CapabilitiesInput {
            id: "ci-runner".into(),
            request_id: None,
        },
        &bus,
    )
    .unwrap();
    let promote = caps
        .capabilities
        .iter()
        .find(|c| c.capability == "can_promote_verified")
        .expect("can_promote_verified row");
    // CI default for can_promote_verified is false.
    assert!(!promote.granted);
    assert!(!promote.overridden);
}
