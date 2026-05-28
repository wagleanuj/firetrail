//! Best-effort event bus used by ops to publish state-change notifications.
//!
//! Adapters (ft-cli for now ignores; ft-ui multiplexes onto an SSE stream)
//! subscribe via [`EventBus::subscribe`]. Emission is non-fatal: if there
//! are no subscribers, or the channel is at capacity, the event is dropped.
//!
//! Every emission is wrapped in an [`EmittedEvent`] envelope that carries
//! an optional `request_id` so the GUI can coalesce optimistic mutations
//! with the matching server-sent event (per the dnd-kit coalescing rule
//! in the design doc).

use serde::Serialize;
use tokio::sync::broadcast::{self, Receiver, Sender};

/// State-change notifications produced by ops.
///
/// Starts intentionally small — Waves 1+ add more variants as the
/// corresponding ops land.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// A ticket was created.
    TicketCreated {
        /// Ticket id.
        id: String,
    },
    /// A ticket transitioned between states.
    TicketTransitioned {
        /// Ticket id.
        id: String,
        /// Previous state.
        from: String,
        /// New state.
        to: String,
    },
    /// A memory record was written.
    MemoryWritten {
        /// Memory id.
        id: String,
    },
}

/// Envelope wrapping every event with a correlation id.
///
/// `request_id` is `None` for events emitted by background tasks; ops invoked
/// through a transport adapter should set it to the inbound request id so the
/// UI can coalesce optimistic state.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize)]
pub struct EmittedEvent {
    /// Correlation id matching the originating request, if any.
    pub request_id: Option<String>,
    /// The event payload.
    pub event: Event,
}

/// Broadcast bus carrying [`EmittedEvent`]s to all subscribers.
#[derive(Debug, Clone)]
pub struct EventBus(Sender<EmittedEvent>);

impl EventBus {
    /// Create a new bus with the given channel capacity. When the channel is
    /// full, the slowest subscriber will lag and miss events (they will see
    /// a `RecvError::Lagged`).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self(tx)
    }

    /// Subscribe to future events. Events emitted before this call are not
    /// replayed.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<EmittedEvent> {
        self.0.subscribe()
    }

    /// Emit an event with no correlation id.
    ///
    /// Best-effort: send errors (no subscribers) are silently ignored.
    pub fn emit(&self, event: Event) {
        let _ = self.0.send(EmittedEvent {
            request_id: None,
            event,
        });
    }

    /// Emit an event tagged with a request correlation id.
    ///
    /// Best-effort: send errors (no subscribers) are silently ignored.
    pub fn emit_with_request(&self, request_id: impl Into<String>, event: Event) {
        let _ = self.0.send(EmittedEvent {
            request_id: Some(request_id.into()),
            event,
        });
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(128)
    }
}
