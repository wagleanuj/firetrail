//! Integration tests for `ft_ops::docs` — the embedded doc surface the ft-ui
//! ticket-drawer Docs panel calls (firetrail-2mwp.8).
//!
//! Mirrors `tests/tickets.rs`: an isolated `TestRepo` with `.firetrail/config.yml`,
//! ops exercised directly (no CLI shell-out). Docs are file-backed — the `.md`
//! on disk is the source of truth, the `Doc` record a thin pointer.

use ft_core::{RecordBody, TrustState};
use ft_ops::docs::{self, AddDocInput, DocFreshnessView, EditDocInput, LinkDocInput};
use ft_ops::tickets::{self, CreateTaskInput, ShowInput};
use ft_ops::{Event, EventBus, Identity, OpsError, Workspace};
use ft_testkit::TestRepo;

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

fn task_defaults(title: &str) -> CreateTaskInput {
    CreateTaskInput {
        title: title.into(),
        description: None,
        epic: None,
        priority: None,
        owner: None,
        scope: None,
        labels: vec![],
        request_id: None,
    }
}

/// Create a task, write `rel_path` with `body`, adopt + link it as a doc.
/// Returns `(task_id, doc_id)`.
fn seed_linked_doc(tr: &TestRepo, ws: &Workspace, rel_path: &str, body: &str) -> (String, String) {
    let id = alice();
    let bus = bus();
    let task = tickets::create_task(ws, &id, task_defaults("documented task"), &bus).unwrap();
    let task_id = task.record.envelope.id.as_str().to_string();

    std::fs::write(tr.root().join(rel_path), body).expect("write doc file");
    let added = docs::add(
        ws,
        &id,
        AddDocInput {
            file: rel_path.into(),
            doc_type: "design".into(),
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("doc add");
    docs::link(
        ws,
        &id,
        LinkDocInput {
            doc: added.id.clone(),
            work_item: task_id.clone(),
        },
        &bus,
    )
    .expect("doc link");
    (task_id, added.id)
}

#[test]
fn docs_for_ticket_returns_linked_doc_rendered_and_fresh() {
    let (tr, ws) = fixture();
    let (task_id, doc_id) =
        seed_linked_doc(&tr, &ws, "design.md", "# Auth design\n\nHow auth works.\n");

    let views = docs::docs_for_ticket(&ws, &alice(), task_id, &bus()).expect("docs_for_ticket");

    assert_eq!(views.len(), 1, "one linked doc");
    let v = &views[0];
    assert_eq!(v.id, doc_id);
    assert_eq!(v.title, "Auth design", "title parsed from H1");
    assert_eq!(v.doc_type, "design");
    assert_eq!(v.path, "design.md");
    assert_eq!(v.freshness, DocFreshnessView::Fresh);
    assert!(
        v.content.contains("How auth works."),
        "content is the raw file: {:?}",
        v.content
    );
}

#[test]
fn out_of_band_edit_marks_doc_stale() {
    let (tr, ws) = fixture();
    let (task_id, _doc_id) = seed_linked_doc(&tr, &ws, "design.md", "# T\n\nOriginal.\n");

    // Edit the file directly (out of band) — the record's content_hash now drifts.
    std::fs::write(tr.root().join("design.md"), "# T\n\nChanged underneath.\n").unwrap();

    let views = docs::docs_for_ticket(&ws, &alice(), task_id, &bus()).unwrap();
    assert_eq!(views[0].freshness, DocFreshnessView::Stale);
    assert!(views[0].content.contains("Changed underneath."));
}

#[test]
fn missing_file_is_a_broken_link() {
    let (tr, ws) = fixture();
    let (task_id, _doc_id) = seed_linked_doc(&tr, &ws, "design.md", "# T\n\nBody.\n");

    std::fs::remove_file(tr.root().join("design.md")).unwrap();

    let views = docs::docs_for_ticket(&ws, &alice(), task_id, &bus()).unwrap();
    assert_eq!(views[0].freshness, DocFreshnessView::Missing);
    assert_eq!(views[0].content, "", "missing file yields empty content");
}

#[test]
fn edit_writes_through_file_and_reindexes_to_fresh() {
    let (tr, ws) = fixture();
    let (task_id, doc_id) = seed_linked_doc(&tr, &ws, "design.md", "# T\n\nOriginal.\n");

    // Drift it first so we can prove edit re-freshens.
    std::fs::write(tr.root().join("design.md"), "# T\n\nDrifted.\n").unwrap();
    let stale = docs::docs_for_ticket(&ws, &alice(), task_id.clone(), &bus()).unwrap();
    assert_eq!(stale[0].freshness, DocFreshnessView::Stale);

    let view = docs::edit(
        &ws,
        &alice(),
        EditDocInput {
            id: doc_id.clone(),
            content: "# T\n\nEdited through the UI.\n".into(),
        },
        &bus(),
    )
    .expect("edit");

    assert_eq!(view.freshness, DocFreshnessView::Fresh);
    assert!(view.content.contains("Edited through the UI."));

    // The file on disk was rewritten.
    let on_disk = std::fs::read_to_string(tr.root().join("design.md")).unwrap();
    assert!(on_disk.contains("Edited through the UI."));

    // And a subsequent read agrees it's fresh (the record was re-indexed).
    let after = docs::docs_for_ticket(&ws, &alice(), task_id, &bus()).unwrap();
    assert_eq!(after[0].freshness, DocFreshnessView::Fresh);
}

/// `edit` publishes a `DocEdited` event so other connected clients can
/// invalidate their cached doc lists and re-derive the freshness badge
/// (firetrail-e4jv).
#[test]
fn edit_emits_doc_edited_event() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();
    std::fs::write(tr.root().join("design.md"), "# Design\n\nThe plan.\n").unwrap();
    let added = docs::add(
        &ws,
        &id,
        AddDocInput {
            file: "design.md".into(),
            doc_type: "design".into(),
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("add doc");

    // Subscribe *after* setup so only the edit's event is observed.
    let mut rx = bus.subscribe();
    docs::edit(
        &ws,
        &id,
        EditDocInput {
            id: added.id.clone(),
            content: "# Design\n\nThe revised plan.\n".into(),
        },
        &bus,
    )
    .expect("edit doc");

    let emitted = rx.try_recv().expect("an event was emitted");
    match emitted.event {
        Event::DocEdited { id: doc_id } => assert_eq!(doc_id, added.id),
        other => panic!("expected DocEdited, got {other:?}"),
    }
}

/// `add` consumes the spec §5 frontmatter: `doc_type`/`scope`/`status`
/// override the call inputs, and each `links:` entry becomes a
/// `work_item --DocumentedIn--> doc` edge — so the doc is reachable from the
/// ticket without a separate `doc link` call (firetrail-5lfs).
#[test]
fn add_applies_frontmatter_and_auto_links() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();
    let task = tickets::create_task(&ws, &id, task_defaults("documented task"), &bus).unwrap();
    let task_id = task.record.envelope.id.as_str().to_string();

    let body = format!(
        "---\n\
         doc_type: adr\n\
         status: reviewed\n\
         scope: ft-embed\n\
         links:\n\
         \x20 - {task_id}\n\
         ---\n\
         # Frontmatter doc\n\nThe prose.\n"
    );
    std::fs::write(tr.root().join("fm.md"), &body).unwrap();

    let added = docs::add(
        &ws,
        &id,
        AddDocInput {
            file: "fm.md".into(),
            doc_type: "design".into(), // frontmatter `adr` must win.
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("doc add");

    // The DocumentedIn edge was created from `links:` — no explicit link call.
    let views = docs::docs_for_ticket(&ws, &id, task_id.clone(), &bus).expect("docs_for_ticket");
    assert_eq!(views.len(), 1, "frontmatter link should make the doc reachable");
    assert_eq!(views[0].id, added.id);
    assert_eq!(views[0].doc_type, "adr", "frontmatter doc_type overrides input");

    // Frontmatter status → trust, and scope → envelope.
    let shown = tickets::show(&ws, &id, ShowInput { id: added.id.clone() }, &bus).expect("show");
    assert_eq!(shown.record.envelope.owning_scope.as_deref(), Some("ft-embed"));
    let RecordBody::Doc(doc) = &shown.record.body else {
        panic!("expected a Doc record");
    };
    assert_eq!(doc.trust, TrustState::Reviewed, "status: reviewed → trust");
    assert_eq!(doc.doc_type, "adr");
}

/// An unresolvable `links:` id is skipped (not an error); valid links still
/// produce edges, and the input `doc_type` stands when frontmatter omits it.
#[test]
fn add_skips_unresolvable_links_and_falls_back_to_input() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();
    let task = tickets::create_task(&ws, &id, task_defaults("real task"), &bus).unwrap();
    let task_id = task.record.envelope.id.as_str().to_string();

    let body = format!(
        "---\n\
         links: [{task_id}, firetrail-doesnotexist]\n\
         ---\n\
         # Doc\n\nProse.\n"
    );
    std::fs::write(tr.root().join("fm.md"), &body).unwrap();

    let added = docs::add(
        &ws,
        &id,
        AddDocInput {
            file: "fm.md".into(),
            doc_type: "runbook".into(),
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("doc add must not fail on an unresolvable link");

    let views = docs::docs_for_ticket(&ws, &id, task_id, &bus).expect("docs_for_ticket");
    assert_eq!(views.len(), 1, "the resolvable link still produced an edge");
    assert_eq!(views[0].id, added.id);
    assert_eq!(
        views[0].doc_type, "runbook",
        "input doc_type stands when frontmatter omits it"
    );

    let shown = tickets::show(&ws, &id, ShowInput { id: added.id }, &bus).expect("show");
    let RecordBody::Doc(doc) = &shown.record.body else {
        panic!("expected a Doc record");
    };
    assert_eq!(doc.trust, TrustState::Draft, "no status → Draft default");
}

// ─── re-index frontmatter refresh (firetrail-izwm, approved Option C) ────────

/// (a) Trust is NEVER refreshed on re-index. An out-of-band edit that changes
/// frontmatter `status:` and a subsequent re-index (via `docs::edit`) must
/// leave the record's trust untouched — the trust ladder stays the sole
/// authority; frontmatter `status:` is consumed only at `doc add`.
#[test]
fn reindex_never_touches_trust_even_when_status_changes() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    // Seed via `add` with frontmatter `status: reviewed` → trust = Reviewed.
    std::fs::write(
        tr.root().join("d.md"),
        "---\nstatus: reviewed\n---\n# D\n\nProse.\n",
    )
    .unwrap();
    let added = docs::add(
        &ws,
        &id,
        AddDocInput {
            file: "d.md".into(),
            doc_type: "design".into(),
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("doc add");

    // Re-index with NEW content whose frontmatter demotes status to draft.
    docs::edit(
        &ws,
        &id,
        EditDocInput {
            id: added.id.clone(),
            content: "---\nstatus: draft\n---\n# D\n\nProse changed.\n".into(),
        },
        &bus,
    )
    .expect("doc edit / re-index");

    let shown = tickets::show(&ws, &id, ShowInput { id: added.id }, &bus).expect("show");
    let RecordBody::Doc(doc) = &shown.record.body else {
        panic!("expected a Doc record");
    };
    assert_eq!(
        doc.trust,
        TrustState::Reviewed,
        "re-index must NOT demote trust from frontmatter status:"
    );
}

/// (b) A new `links:` entry added out of band is reconciled on re-index: the
/// new `DocumentedIn` edge is created and any pre-existing edge remains.
#[test]
fn reindex_adds_new_link_edge_and_keeps_existing() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    let task_a = tickets::create_task(&ws, &id, task_defaults("task a"), &bus).unwrap();
    let task_a_id = task_a.record.envelope.id.as_str().to_string();
    let other_task = tickets::create_task(&ws, &id, task_defaults("task b"), &bus).unwrap();
    let other_id = other_task.record.envelope.id.as_str().to_string();

    // Seed a doc already linked (via frontmatter) to task A.
    let body = format!("---\nlinks:\n  - {task_a_id}\n---\n# D\n\nProse.\n");
    std::fs::write(tr.root().join("d.md"), &body).unwrap();
    let added = docs::add(
        &ws,
        &id,
        AddDocInput {
            file: "d.md".into(),
            doc_type: "design".into(),
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("doc add");

    // Out-of-band edit ADDS task B to links: (keeps A). Re-index via edit.
    let new_body =
        format!("---\nlinks:\n  - {task_a_id}\n  - {other_id}\n---\n# D\n\nProse changed.\n");
    docs::edit(
        &ws,
        &id,
        EditDocInput {
            id: added.id.clone(),
            content: new_body,
        },
        &bus,
    )
    .expect("doc edit / re-index");

    // Both tickets now reach the same doc via DocumentedIn.
    let a_views = docs::docs_for_ticket(&ws, &id, task_a_id, &bus).expect("docs A");
    assert_eq!(a_views.len(), 1, "pre-existing edge to A remains");
    assert_eq!(a_views[0].id, added.id);
    let b_views = docs::docs_for_ticket(&ws, &id, other_id, &bus).expect("docs B");
    assert_eq!(b_views.len(), 1, "new edge to B was created on re-index");
    assert_eq!(b_views[0].id, added.id);
}

/// (c) `doc_type` (body) and `scope` (envelope) ARE refreshed from frontmatter
/// on re-index.
#[test]
fn reindex_refreshes_doc_type_and_scope() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    std::fs::write(
        tr.root().join("d.md"),
        "---\ndoc_type: design\nscope: ft-ops\n---\n# D\n\nProse.\n",
    )
    .unwrap();
    let added = docs::add(
        &ws,
        &id,
        AddDocInput {
            file: "d.md".into(),
            doc_type: "design".into(),
            title: None,
            scope: None,
        },
        &bus,
    )
    .expect("doc add");

    docs::edit(
        &ws,
        &id,
        EditDocInput {
            id: added.id.clone(),
            content: "---\ndoc_type: adr\nscope: ft-embed\n---\n# D\n\nProse changed.\n".into(),
        },
        &bus,
    )
    .expect("doc edit / re-index");

    let shown = tickets::show(&ws, &id, ShowInput { id: added.id }, &bus).expect("show");
    assert_eq!(
        shown.record.envelope.owning_scope.as_deref(),
        Some("ft-embed"),
        "scope refreshed on re-index"
    );
    let RecordBody::Doc(doc) = &shown.record.body else {
        panic!("expected a Doc record");
    };
    assert_eq!(doc.doc_type, "adr", "doc_type refreshed on re-index");
}

/// (d) An edge created via explicit `doc link` is NOT removed when re-indexing
/// a doc whose frontmatter omits that work item (re-index is additive only).
#[test]
fn reindex_does_not_remove_explicit_link_edge() {
    let (tr, ws) = fixture();
    let id = alice();
    let bus = bus();

    // Seed a doc with NO frontmatter links, then link it explicitly.
    let (task_id, doc_id) = seed_linked_doc(&tr, &ws, "d.md", "# D\n\nProse.\n");

    // Sanity: the explicit link is reachable.
    let before = docs::docs_for_ticket(&ws, &id, task_id.clone(), &bus).expect("docs before");
    assert_eq!(before.len(), 1);

    // Re-index with content whose frontmatter omits the work item entirely.
    docs::edit(
        &ws,
        &id,
        EditDocInput {
            id: doc_id.clone(),
            content: "---\ndoc_type: adr\n---\n# D\n\nNo links here.\n".into(),
        },
        &bus,
    )
    .expect("doc edit / re-index");

    let after = docs::docs_for_ticket(&ws, &id, task_id, &bus).expect("docs after");
    assert_eq!(
        after.len(),
        1,
        "explicit doc-link edge must survive a re-index that omits it"
    );
    assert_eq!(after[0].id, doc_id);
}

#[test]
fn edit_rejects_non_doc_record() {
    let (_tr, ws) = fixture();
    let id = alice();
    let bus = bus();
    let task = tickets::create_task(&ws, &id, task_defaults("not a doc"), &bus).unwrap();
    let task_id = task.record.envelope.id.as_str().to_string();

    let err = docs::edit(
        &ws,
        &id,
        EditDocInput {
            id: task_id,
            content: "whatever".into(),
        },
        &bus,
    )
    .expect_err("editing a non-doc record must fail");
    assert!(matches!(err, OpsError::Validation { .. }), "{err:?}");
}
