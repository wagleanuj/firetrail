//! Tests for the file-backed `Doc` record kind (firetrail-2mwp.2).
//!
//! A `Doc` is a thin pointer to an external `.md` file: the file is the source
//! of truth, the record carries `path`, `content_hash`, a `summary` excerpt,
//! an open `doc_type` tag, and `trust`. These exercise serde round-trip, schema
//! validation, `state_hash` recomputation, the builder convenience setter, and
//! that the doc↔work relation kinds are part of the writable subset.

use ft_core::{
    Doc, Identity, Origin, Record, RecordBody, RecordBuilder, RecordKind, RelationKind, TrustState,
    hash::state_hash, validate_record_json,
};

fn alice() -> Identity {
    Identity::new("alice@example.com").unwrap()
}

fn sample_doc() -> Doc {
    Doc {
        path: "docs/superpowers/specs/2026-05-29-firetrail-docs-design.md".into(),
        content_hash: "b3:deadbeef".into(),
        title: "Firetrail Docs design".into(),
        summary: "File-backed design/arch docs linked to work items.".into(),
        doc_type: "design".into(),
        trust: TrustState::Reviewed,
    }
}

#[test]
fn doc_roundtrips_validates_and_hash_matches() {
    let r = RecordBuilder::new(RecordKind::Doc, "Firetrail Docs design", alice())
        .doc(sample_doc())
        .origin(Origin::Human)
        .build()
        .expect("doc record must build");

    assert!(matches!(r.body, RecordBody::Doc(_)));
    assert_eq!(r.envelope.kind, RecordKind::Doc);
    assert!(r.envelope.id.as_str().starts_with("DOC-"));

    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back, "serde round-trip must be lossless");

    validate_record_json(&serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(state_hash(&r).unwrap(), r.envelope.state_hash);
}

#[test]
fn doc_defaults_trust_to_draft_when_missing_on_disk() {
    let r = RecordBuilder::new(RecordKind::Doc, "d", alice())
        .doc(Doc::default())
        .build()
        .unwrap();
    let mut v = serde_json::to_value(&r).unwrap();
    v.get_mut("body")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("trust");
    let back: Record = serde_json::from_value(v).expect("must accept missing trust");
    match back.body {
        RecordBody::Doc(d) => assert_eq!(d.trust, TrustState::Draft),
        other => panic!("expected Doc, got {other:?}"),
    }
}

#[test]
fn doc_type_is_an_open_tag() {
    // Non-conventional values must round-trip — doc_type is a free String, not
    // an enum, so teams can use custom taxonomies.
    let mut doc = sample_doc();
    doc.doc_type = "playbook-2026".into();
    let r = RecordBuilder::new(RecordKind::Doc, "x", alice())
        .doc(doc)
        .build()
        .unwrap();
    let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    match back.body {
        RecordBody::Doc(d) => assert_eq!(d.doc_type, "playbook-2026"),
        other => panic!("expected Doc, got {other:?}"),
    }
}

#[test]
fn documented_in_and_implemented_by_are_writable() {
    // The doc↔work relation kinds must be part of the writable subset so
    // `firetrail doc link` can create them.
    assert!(RelationKind::DocumentedIn.writable_in_m1());
    assert!(RelationKind::ImplementedBy.writable_in_m1());
}
