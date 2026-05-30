//! Smoke test for the Wave 0 ft-ops scaffold.

use std::fs;

use ft_ops::workspace::WorkspaceError;
use ft_ops::{Event, EventBus, OpsError, Workspace};

#[test]
fn workspace_open_requires_marker() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Without the marker, opening must fail.
    let err = Workspace::open(root).expect_err("expected open to fail without marker");
    assert!(
        matches!(err, WorkspaceError::Validation { .. }),
        "expected Validation error, got {err:?}"
    );

    // Create the marker ft-cli uses (`.firetrail/config.yml`) and try again.
    fs::create_dir_all(root.join(".firetrail")).expect("mkdir .firetrail");
    fs::write(root.join(".firetrail").join("config.yml"), "version: 0\n").expect("write marker");
    let ws = Workspace::open(root).expect("workspace opens with marker present");
    assert_eq!(ws.root, root);
    assert_eq!(ws.firetrail_dir(), root.join(".firetrail"));
}

#[tokio::test]
async fn event_bus_round_trip() {
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();

    bus.emit(Event::TicketCreated { id: "T-1".into() });

    let envelope = rx.recv().await.expect("receive event");
    assert!(envelope.request_id.is_none());
    match envelope.event {
        Event::TicketCreated { id } => assert_eq!(id, "T-1"),
        other => panic!("unexpected event: {other:?}"),
    }

    // Tagged emission preserves the request id.
    bus.emit_with_request("req-42", Event::MemoryWritten { id: "mem-1".into() });
    let envelope = rx.recv().await.expect("receive tagged event");
    assert_eq!(envelope.request_id.as_deref(), Some("req-42"));
}

#[test]
fn not_found_display_is_sensible() {
    let err = OpsError::not_found("ticket", "T-99");
    assert_eq!(err.to_string(), "ticket not found: T-99");
}
