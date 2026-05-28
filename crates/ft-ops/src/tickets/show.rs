//! Read a single ticket and its inbound/outbound relations.

use ft_core::{Record, Relation};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::{TicketCtx, load_relations};

/// Input for [`show`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShowInput {
    /// Ticket id (full canonical form or unambiguous prefix).
    pub id: String,
}

/// Output of [`show`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShowOutput {
    /// The resolved record.
    pub record: Record,
    /// Every relation in the workspace whose endpoints touch this id.
    pub relations: Vec<Relation>,
}

/// `show` op — fetch a ticket plus the relations that reference it.
///
/// Read-only; emits no events.
#[allow(clippy::needless_pass_by_value)]
pub fn show(
    ws: &Workspace,
    identity: &Identity,
    input: ShowInput,
    _events: &EventBus,
) -> Result<ShowOutput, OpsError> {
    let ctx = TicketCtx::open(ws, identity, "show")?;
    let id = ctx.resolve_id(&input.id)?;
    let record = ctx.read_record(&id)?;

    let all = load_relations(ws)?;
    let relations: Vec<Relation> = all
        .into_iter()
        .filter(|r| r.from == id || r.to == id)
        .collect();

    Ok(ShowOutput { record, relations })
}
