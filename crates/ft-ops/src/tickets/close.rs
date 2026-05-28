//! Close a ticket. Validates acceptance criteria unless `force` is set.

use chrono::Utc;
use ft_core::{AcStatus, AcceptanceCriterion, Label, Record, RecordBody, Status};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::TicketCtx;

/// Input for [`close`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloseInput {
    /// Ticket id (full canonical or unambiguous prefix).
    pub id: String,
    /// Skip acceptance-criteria validation. Requires `reason`.
    #[serde(default)]
    pub force: bool,
    /// Reason for forcing the close. Required when `force` is `true`; recorded
    /// as a `force_close_reason` label on the envelope.
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional client-supplied correlation id; propagated onto every
    /// emitted [`crate::Event`] envelope.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`close`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloseOutput {
    /// The updated record.
    pub record: Record,
}

/// `close` op — transition a ticket to `Status::Closed`.
///
/// Fails with [`OpsError::Conflict`] when the AC gate is violated and
/// `force` is not set; fails with [`OpsError::Validation`] when `force` is set
/// without a `reason`.
#[allow(clippy::needless_pass_by_value)]
pub fn close(
    ws: &Workspace,
    identity: &Identity,
    input: CloseInput,
    events: &EventBus,
) -> Result<CloseOutput, OpsError> {
    // Input-level validation first so the caller gets a deterministic error
    // regardless of record state.
    if input.force && input.reason.as_deref().map_or("", str::trim).is_empty() {
        return Err(OpsError::validation("reason", "--force requires --reason"));
    }

    let mut ctx = TicketCtx::open(ws, identity, "close")?;
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;

    if record.envelope.status == Status::Closed {
        return Err(OpsError::Conflict {
            reason: "record is already closed".to_string(),
        });
    }

    let incomplete = unchecked_criteria(&record.body);
    if !incomplete.is_empty() && !input.force {
        return Err(OpsError::Conflict {
            reason: format!("{} acceptance criteria are incomplete", incomplete.len()),
        });
    }

    if input.force {
        let reason = input
            .reason
            .clone()
            .ok_or_else(|| OpsError::validation("reason", "--force requires --reason"))?;
        record.envelope.labels.push(Label {
            key: "force_close_reason".into(),
            value: reason,
        });
    }

    let from = record.envelope.status;
    record.envelope.status = Status::Closed;
    let now = Utc::now();
    record.envelope.closed_at = Some(now);
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;

    let id_str = record.envelope.id.as_str().to_string();
    let rid = input.request_id.as_deref();
    emit(
        events,
        rid,
        Event::TicketTransitioned {
            id: id_str.clone(),
            from: status_str(from),
            to: status_str(Status::Closed),
        },
    );
    emit(events, rid, Event::TicketClosed { id: id_str });

    Ok(CloseOutput { record })
}

/// Emit `event` on `bus`, threading `request_id` when present.
fn emit(bus: &EventBus, request_id: Option<&str>, event: Event) {
    if let Some(rid) = request_id {
        bus.emit_with_request(rid.to_string(), event);
    } else {
        bus.emit(event);
    }
}

fn unchecked_criteria(body: &RecordBody) -> Vec<&AcceptanceCriterion> {
    let acs: &[AcceptanceCriterion] = match body {
        RecordBody::Task(t) => &t.acceptance_criteria,
        RecordBody::Subtask(s) => &s.acceptance_criteria,
        RecordBody::Bug(b) => &b.acceptance_criteria,
        _ => &[],
    };
    acs.iter()
        .filter(|a| a.status != AcStatus::Checked)
        .collect()
}

fn status_str(s: Status) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{s:?}"))
}
