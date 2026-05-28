//! Ticket creation ops — epic / task / subtask / bug.
//!
//! Each `create_*` op resolves the actor, builds the record body, runs it
//! through `ft_core::RecordBuilder`, persists via storage, refreshes the
//! index, and publishes a `TicketCreated` event.

use ft_core::{
    Bug, Epic, Identity as CoreIdentity, Label, Priority, Record, RecordBuilder, RecordKind,
    Subtask, Task,
};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::TicketCtx;

/// Priority selector used in create / update inputs.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TicketPriority {
    /// Critical (top-of-queue).
    P0,
    /// High priority.
    P1,
    /// Normal priority.
    P2,
    /// Low priority.
    P3,
    /// Backlog.
    P4,
}

impl TicketPriority {
    fn to_core(self) -> Priority {
        match self {
            Self::P0 => Priority::P0,
            Self::P1 => Priority::P1,
            Self::P2 => Priority::P2,
            Self::P3 => Priority::P3,
            Self::P4 => Priority::P4,
        }
    }
}

/// Successful creation output — the freshly written record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedTicket {
    /// The new record (envelope + body).
    pub record: Record,
}

/// Input for [`create_epic`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateEpicInput {
    /// Title (required, non-empty).
    pub title: String,
    /// Free-form description (markdown).
    #[serde(default)]
    pub description: Option<String>,
    /// Priority.
    #[serde(default)]
    pub priority: Option<TicketPriority>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// `key=value` labels. Each entry must contain exactly one `=`.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// `create epic` op.
pub fn create_epic(
    ws: &Workspace,
    identity: &Identity,
    input: CreateEpicInput,
    events: &EventBus,
) -> Result<CreatedTicket, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "epic create")?;
    let actor = ctx.actor.clone();

    let body = Epic {
        description: input.description.unwrap_or_default(),
        child_ids: Vec::new(),
    };
    let builder = builder_with_common(
        RecordKind::Epic,
        &input.title,
        actor,
        input.priority.map(TicketPriority::to_core),
        input.scope.as_deref(),
    )
    .epic(body);
    let mut record = build_with_labels(builder, &input.labels)?;
    ctx.save_record(&mut record)?;
    events.emit(Event::TicketCreated {
        id: record.envelope.id.as_str().to_string(),
    });
    Ok(CreatedTicket { record })
}

/// Input for [`create_task`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateTaskInput {
    /// Title (required, non-empty).
    pub title: String,
    /// Free-form description (markdown).
    #[serde(default)]
    pub description: Option<String>,
    /// Parent epic id (full canonical or unambiguous prefix). Must resolve to
    /// an `Epic`.
    #[serde(default)]
    pub epic: Option<String>,
    /// Priority.
    #[serde(default)]
    pub priority: Option<TicketPriority>,
    /// Owner identity.
    #[serde(default)]
    pub owner: Option<String>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// `key=value` labels.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// `create task` op.
pub fn create_task(
    ws: &Workspace,
    identity: &Identity,
    input: CreateTaskInput,
    events: &EventBus,
) -> Result<CreatedTicket, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "task create")?;
    let actor = ctx.actor.clone();

    let parent_epic = if let Some(raw) = input.epic.as_deref() {
        let id = ctx.resolve_id(raw)?;
        if id.kind() != RecordKind::Epic {
            return Err(OpsError::validation(
                "epic",
                format!("--epic {id} is not an epic"),
            ));
        }
        Some(id)
    } else {
        None
    };

    let body = Task {
        description: input.description.unwrap_or_default(),
        parent_epic,
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        claim: None,
    };
    let mut builder = builder_with_common(
        RecordKind::Task,
        &input.title,
        actor,
        input.priority.map(TicketPriority::to_core),
        input.scope.as_deref(),
    )
    .task(body);
    if let Some(owner) = input.owner {
        let id = CoreIdentity::new(owner)
            .map_err(|e| OpsError::validation("owner", e.to_string()))?;
        builder = builder.owner(id);
    }
    let mut record = build_with_labels(builder, &input.labels)?;
    ctx.save_record(&mut record)?;
    events.emit(Event::TicketCreated {
        id: record.envelope.id.as_str().to_string(),
    });
    Ok(CreatedTicket { record })
}

/// Input for [`create_subtask`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateSubtaskInput {
    /// Title (required, non-empty).
    pub title: String,
    /// Parent task id (required). Must resolve to a `Task`.
    pub parent: String,
    /// Free-form description (markdown).
    #[serde(default)]
    pub description: Option<String>,
    /// Priority.
    #[serde(default)]
    pub priority: Option<TicketPriority>,
    /// Owner identity.
    #[serde(default)]
    pub owner: Option<String>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// `key=value` labels.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// `create subtask` op.
pub fn create_subtask(
    ws: &Workspace,
    identity: &Identity,
    input: CreateSubtaskInput,
    events: &EventBus,
) -> Result<CreatedTicket, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "subtask create")?;
    let actor = ctx.actor.clone();

    let parent = ctx.resolve_id(&input.parent)?;
    if parent.kind() != RecordKind::Task {
        return Err(OpsError::validation(
            "parent",
            format!("--parent {parent} is not a task"),
        ));
    }

    let body = Subtask {
        description: input.description.unwrap_or_default(),
        parent_task: parent,
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        claim: None,
    };
    let mut builder = builder_with_common(
        RecordKind::Subtask,
        &input.title,
        actor,
        input.priority.map(TicketPriority::to_core),
        input.scope.as_deref(),
    )
    .subtask(body);
    if let Some(owner) = input.owner {
        let id = CoreIdentity::new(owner)
            .map_err(|e| OpsError::validation("owner", e.to_string()))?;
        builder = builder.owner(id);
    }
    let mut record = build_with_labels(builder, &input.labels)?;
    ctx.save_record(&mut record)?;
    events.emit(Event::TicketCreated {
        id: record.envelope.id.as_str().to_string(),
    });
    Ok(CreatedTicket { record })
}

/// Input for [`create_bug`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateBugInput {
    /// Title (required, non-empty).
    pub title: String,
    /// Free-form description (markdown).
    #[serde(default)]
    pub description: Option<String>,
    /// Affected service.
    #[serde(default)]
    pub service: Option<String>,
    /// Free-form severity tag (e.g. `"sev2"`).
    #[serde(default)]
    pub severity: Option<String>,
    /// Priority.
    #[serde(default)]
    pub priority: Option<TicketPriority>,
    /// Owning scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// `key=value` labels.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// `create bug` op.
pub fn create_bug(
    ws: &Workspace,
    identity: &Identity,
    input: CreateBugInput,
    events: &EventBus,
) -> Result<CreatedTicket, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "bug create")?;
    let actor = ctx.actor.clone();

    let body = Bug {
        description: input.description.unwrap_or_default(),
        service: input.service,
        severity: input.severity,
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        claim: None,
    };
    let builder = builder_with_common(
        RecordKind::Bug,
        &input.title,
        actor,
        input.priority.map(TicketPriority::to_core),
        input.scope.as_deref(),
    )
    .bug(body);
    let mut record = build_with_labels(builder, &input.labels)?;
    ctx.save_record(&mut record)?;
    events.emit(Event::TicketCreated {
        id: record.envelope.id.as_str().to_string(),
    });
    Ok(CreatedTicket { record })
}

fn builder_with_common(
    kind: RecordKind,
    title: &str,
    created_by: CoreIdentity,
    priority: Option<Priority>,
    scope: Option<&str>,
) -> RecordBuilder {
    let mut b = RecordBuilder::new(kind, title, created_by);
    if let Some(p) = priority {
        b = b.priority(p);
    }
    if let Some(s) = scope {
        b = b.owning_scope(s);
    }
    b
}

/// Parse `key=value` labels, build the record, then re-apply labels to the
/// envelope (the builder does not expose a label setter).
fn build_with_labels(builder: RecordBuilder, labels: &[String]) -> Result<Record, OpsError> {
    let parsed: Vec<(String, String)> = labels
        .iter()
        .map(|raw| parse_label_pair(raw))
        .collect::<Result<_, _>>()?;
    let mut record = builder
        .build()
        .map_err(|e| OpsError::validation("record", e.to_string()))?;
    for (key, value) in parsed {
        record.envelope.labels.push(Label { key, value });
    }
    Ok(record)
}

fn parse_label_pair(raw: &str) -> Result<(String, String), OpsError> {
    let (k, v) = raw.split_once('=').ok_or_else(|| {
        OpsError::validation(
            "labels",
            format!("label `{raw}` must be in `key=value` form"),
        )
    })?;
    if k.trim().is_empty() {
        return Err(OpsError::validation(
            "labels",
            format!("label key in `{raw}` is empty"),
        ));
    }
    Ok((k.trim().to_string(), v.trim().to_string()))
}
