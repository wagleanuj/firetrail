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
#[non_exhaustive]
pub enum Event {
    /// A ticket was created.
    TicketCreated {
        /// Ticket id.
        id: String,
    },
    /// A ticket envelope (title / priority / owner / description) was updated.
    TicketUpdated {
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
    /// A ticket was claimed.
    TicketClaimed {
        /// Ticket id.
        id: String,
        /// Identity that holds the claim.
        actor: String,
    },
    /// A claim on a ticket was released.
    TicketUnclaimed {
        /// Ticket id.
        id: String,
    },
    /// A ticket was closed.
    TicketClosed {
        /// Ticket id.
        id: String,
    },
    /// A relation was added between two tickets.
    TicketLinked {
        /// Source ticket id.
        from: String,
        /// Target ticket id.
        to: String,
        /// Relation kind (serialized form, e.g. `"blocked-by"`).
        relation: String,
    },
    /// A memory record was written.
    ///
    /// Emitted on **every** memory write (create or update). Kept distinct
    /// from [`Event::MemoryCreated`] so transports that only care about new
    /// records can subscribe narrowly while audit/telemetry consumers can
    /// listen for any change. Wave 2-A only emits [`Event::MemoryCreated`];
    /// `MemoryWritten` is reserved for memory-update ops landing under a
    /// later wave (trust transitions, redact, etc.).
    MemoryWritten {
        /// Memory id.
        id: String,
    },
    /// A new memory record was created (Wave 2-A).
    MemoryCreated {
        /// Memory id.
        id: String,
        /// Record kind (lowercase, e.g. `"incident"`, `"finding"`, …). Named
        /// `record_kind` rather than `kind` because the parent enum uses
        /// `kind` as its serde discriminator.
        record_kind: String,
    },
    /// A memory record was processed by the salvage workflow (Wave 2-A).
    MemorySalvaged {
        /// Memory id.
        id: String,
        /// Operator decision for this record.
        decision: SalvageDecision,
    },
}

/// Per-record outcome of the salvage workflow.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SalvageDecision {
    /// Record was copied onto the salvage branch.
    Accepted,
    /// Record was deliberately skipped.
    Rejected,
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
