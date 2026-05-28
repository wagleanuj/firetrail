//! Envelope-field updates on an existing ticket.

use chrono::Utc;
use ft_core::{Identity as CoreIdentity, Record, RecordBody, Status};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::TicketCtx;
use super::create::TicketPriority;
use super::list::TicketStatusFilter;

/// Input for [`update`].
///
/// Every field is optional; the op fails with [`OpsError::Validation`] if no
/// fields are supplied (matching the CLI's "at least one of …" behaviour).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateInput {
    /// Ticket id (full canonical or unambiguous prefix).
    pub id: String,
    /// New title. Trimmed; rejected if empty.
    #[serde(default)]
    pub title: Option<String>,
    /// New status.
    #[serde(default)]
    pub status: Option<TicketStatusFilter>,
    /// New priority.
    #[serde(default)]
    pub priority: Option<TicketPriority>,
    /// New owner identity. Pass an empty / whitespace-only string to clear.
    #[serde(default)]
    pub owner: Option<String>,
    /// New description. Only valid on epic / task / subtask / bug.
    #[serde(default)]
    pub description: Option<String>,
}

/// Output of [`update`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateOutput {
    /// The updated record.
    pub record: Record,
    /// Previous status (only set when `--status` was applied).
    pub previous_status: Option<String>,
}

/// `update` op — mutate envelope fields on an existing ticket.
pub fn update(
    ws: &Workspace,
    identity: &Identity,
    input: UpdateInput,
    events: &EventBus,
) -> Result<UpdateOutput, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "update")?;
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;

    let mut touched = false;
    let mut prev_status: Option<Status> = None;
    let mut new_status: Option<Status> = None;

    if let Some(t) = input.title {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            return Err(OpsError::validation("title", "title cannot be empty"));
        }
        record.envelope.title = trimmed.to_string();
        touched = true;
    }
    if let Some(s) = input.status {
        let new = ticket_status_to_core(s);
        prev_status = Some(record.envelope.status);
        if new == Status::Closed && record.envelope.status != Status::Closed {
            record.envelope.closed_at = Some(Utc::now());
        }
        if new != Status::Closed {
            record.envelope.closed_at = None;
        }
        record.envelope.status = new;
        new_status = Some(new);
        touched = true;
    }
    if let Some(p) = input.priority {
        record.envelope.priority = match p {
            TicketPriority::P0 => ft_core::Priority::P0,
            TicketPriority::P1 => ft_core::Priority::P1,
            TicketPriority::P2 => ft_core::Priority::P2,
            TicketPriority::P3 => ft_core::Priority::P3,
            TicketPriority::P4 => ft_core::Priority::P4,
        };
        touched = true;
    }
    if let Some(owner) = input.owner {
        let trimmed = owner.trim();
        if trimmed.is_empty() {
            record.envelope.owner = None;
        } else {
            let id = CoreIdentity::new(trimmed)
                .map_err(|e| OpsError::validation("owner", e.to_string()))?;
            record.envelope.owner = Some(id);
        }
        touched = true;
    }
    if let Some(d) = input.description {
        match &mut record.body {
            RecordBody::Epic(e) => e.description = d,
            RecordBody::Task(t) => t.description = d,
            RecordBody::Subtask(s) => s.description = d,
            RecordBody::Bug(b) => b.description = d,
            other => {
                return Err(OpsError::validation(
                    "description",
                    format!(
                        "--description is not supported for {:?} records",
                        other.kind()
                    ),
                ));
            }
        }
        touched = true;
    }

    if !touched {
        return Err(OpsError::validation(
            "input",
            "no fields to update; supply at least one of title, status, priority, owner, description",
        ));
    }

    record.envelope.updated_at = Utc::now();
    ctx.save_record(&mut record)?;

    let id_str = record.envelope.id.as_str().to_string();
    if let (Some(from), Some(to)) = (prev_status, new_status) {
        if from != to {
            events.emit(Event::TicketTransitioned {
                id: id_str.clone(),
                from: status_str(from),
                to: status_str(to),
            });
        }
    }
    events.emit(Event::TicketUpdated { id: id_str });

    Ok(UpdateOutput {
        record,
        previous_status: prev_status.map(status_str),
    })
}

fn ticket_status_to_core(s: TicketStatusFilter) -> Status {
    match s {
        TicketStatusFilter::Open => Status::Open,
        TicketStatusFilter::Ready => Status::Ready,
        TicketStatusFilter::InProgress => Status::InProgress,
        TicketStatusFilter::Review => Status::Review,
        TicketStatusFilter::Blocked => Status::Blocked,
        TicketStatusFilter::Closed => Status::Closed,
        TicketStatusFilter::Deferred => Status::Deferred,
        TicketStatusFilter::Archived => Status::Archived,
    }
}

fn status_str(s: Status) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{s:?}"))
}
