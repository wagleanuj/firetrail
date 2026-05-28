//! Create a relation edge between two tickets.
//!
//! Edges are persisted to the interim relation log
//! (`.firetrail/relations.jsonl`) and surfaced to subsequent queries via an
//! index refresh.

use chrono::Utc;
use ft_core::{RecordId, Relation, RelationKind};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::{TicketCtx, append_relation, relation_kind_str};

/// Wire form of [`ft_core::RelationKind`].
///
/// Mirrors the kebab-case serde representation of the core enum.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TicketRelationKind {
    /// Blocks.
    Blocks,
    /// Blocked by.
    BlockedBy,
    /// Parent of.
    ParentOf,
    /// Child of.
    ChildOf,
    /// Related to.
    RelatedTo,
    /// Duplicates.
    Duplicates,
    /// Supersedes.
    Supersedes,
    /// Fixed by.
    FixedBy,
    /// Caused by.
    CausedBy,
}

impl TicketRelationKind {
    fn to_core(self) -> RelationKind {
        match self {
            Self::Blocks => RelationKind::Blocks,
            Self::BlockedBy => RelationKind::BlockedBy,
            Self::ParentOf => RelationKind::ParentOf,
            Self::ChildOf => RelationKind::ChildOf,
            Self::RelatedTo => RelationKind::RelatedTo,
            Self::Duplicates => RelationKind::Duplicates,
            Self::Supersedes => RelationKind::Supersedes,
            Self::FixedBy => RelationKind::FixedBy,
            Self::CausedBy => RelationKind::CausedBy,
        }
    }
}

/// Input for [`link`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkInput {
    /// Source ticket id (full canonical or unambiguous prefix).
    pub from: String,
    /// Target ticket id.
    pub to: String,
    /// Relation kind.
    pub kind: TicketRelationKind,
    /// Optional client-supplied correlation id; propagated onto the
    /// emitted [`crate::Event::TicketLinked`] envelope.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`link`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkOutput {
    /// Resolved source canonical id.
    pub from: RecordId,
    /// Resolved target canonical id.
    pub to: RecordId,
    /// Stored relation kind.
    pub kind: RelationKind,
}

/// `link` op — append a relation edge between two existing tickets.
#[allow(clippy::needless_pass_by_value)]
pub fn link(
    ws: &Workspace,
    identity: &Identity,
    input: LinkInput,
    events: &EventBus,
) -> Result<LinkOutput, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "link")?;
    let from = ctx.resolve_id(&input.from)?;
    let to = ctx.resolve_id(&input.to)?;
    if from == to {
        return Err(OpsError::validation("to", "self-edges are not allowed"));
    }
    let actor = ctx.actor.clone();
    let core_kind = input.kind.to_core();
    // Refuse if endpoints don't exist.
    let _ = ctx.read_record(&from)?;
    let _ = ctx.read_record(&to)?;

    let relation = Relation {
        from: from.clone(),
        to: to.clone(),
        kind: core_kind,
        created_at: Utc::now(),
        created_by: actor,
    };
    append_relation(ws, &relation)?;
    ctx.index
        .refresh(&ctx.storage, &[], &[])
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("index refresh: {e}")))?;

    let event = Event::TicketLinked {
        from: from.as_str().to_string(),
        to: to.as_str().to_string(),
        relation: relation_kind_str(core_kind),
    };
    if let Some(rid) = input.request_id.as_deref() {
        events.emit_with_request(rid.to_string(), event);
    } else {
        events.emit(event);
    }

    Ok(LinkOutput {
        from,
        to,
        kind: core_kind,
    })
}
