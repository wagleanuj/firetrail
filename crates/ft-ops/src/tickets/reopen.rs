//! Reopen a closed (or deferred) ticket — transitions the record back to
//! `Status::Open`. Mirrors the CLI's `firetrail reopen` command.

use chrono::Utc;
use ft_core::{Record, Status};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::{TicketCtx, status_str};

/// Input for [`reopen`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReopenInput {
    /// Ticket id (full canonical or unambiguous prefix).
    pub id: String,
    /// Optional client-supplied correlation id; propagated onto every
    /// emitted [`crate::Event`] envelope.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`reopen`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReopenOutput {
    /// The updated record.
    pub record: Record,
}

/// `reopen` op — transition a closed/deferred ticket back to `Status::Open`.
///
/// Fails with [`OpsError::Conflict`] when the record is not in a closed or
/// deferred state.
#[allow(clippy::needless_pass_by_value)]
pub fn reopen(
    ws: &Workspace,
    identity: &Identity,
    input: ReopenInput,
    events: &EventBus,
) -> Result<ReopenOutput, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "reopen")?;
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;

    if record.envelope.status != Status::Closed && record.envelope.status != Status::Deferred {
        return Err(OpsError::Conflict {
            reason: "record is not in a closed/deferred state".to_string(),
        });
    }

    let from = record.envelope.status;
    record.envelope.status = Status::Open;
    record.envelope.closed_at = None;
    record.envelope.updated_at = Utc::now();
    ctx.save_record(&mut record)?;

    let id_str = record.envelope.id.as_str().to_string();
    let rid = input.request_id.as_deref();
    emit(
        events,
        rid,
        Event::TicketTransitioned {
            id: id_str.clone(),
            from: status_str(from),
            to: status_str(Status::Open),
        },
    );

    Ok(ReopenOutput { record })
}

fn emit(bus: &EventBus, request_id: Option<&str>, event: Event) {
    if let Some(rid) = request_id {
        bus.emit_with_request(rid.to_string(), event);
    } else {
        bus.emit(event);
    }
}
