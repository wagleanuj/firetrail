//! `firetrail {epic,task,subtask,bug} create` — write a new record.
//!
//! Each handler shares the same skeleton: resolve identity, build the body,
//! run it through `RecordBuilder`, persist via storage, refresh the index,
//! return the new record.

use ft_core::{Bug, Epic, Identity, Label, Priority, RecordBuilder, RecordKind, Subtask, Task};

use crate::cli::{CreateBugArgs, CreateRecordArgs, CreateSubtaskArgs, CreateTaskArgs, GlobalOpts};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::{WorkCtx, parse_label_pair};
use crate::error::CliError;

const CMD_EPIC: &str = "epic create";
const CMD_TASK: &str = "task create";
const CMD_SUBTASK: &str = "subtask create";
const CMD_BUG: &str = "bug create";

/// `firetrail epic create`
pub fn epic(args: &CreateRecordArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_EPIC, global.workspace.as_deref())?;
    let actor = ctx.actor()?;

    let body = Epic {
        description: args.description.clone().unwrap_or_default(),
        child_ids: Vec::new(),
    };
    let mut builder = builder_with_common(
        RecordKind::Epic,
        &args.title,
        actor,
        args.priority.map(super::priority_to_core),
        args.scope.as_deref(),
    )
    .epic(body);
    builder = apply_labels(builder, &args.labels, CMD_EPIC)?;
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_EPIC, e.to_string()))?;
    apply_label_envelope(&mut record, &args.labels, CMD_EPIC)?;
    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Created(RecordOutcome {
        command: CMD_EPIC,
        record,
    }))
}

/// `firetrail task create`
pub fn task(args: &CreateTaskArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_TASK, global.workspace.as_deref())?;
    let actor = ctx.actor()?;

    let parent_epic = args
        .epic
        .as_deref()
        .map(|raw| ctx.resolve_id(raw))
        .transpose()?;
    if let Some(p) = &parent_epic {
        if p.kind() != RecordKind::Epic {
            return Err(CliError::user(
                CMD_TASK,
                format!("--epic {p} is not an epic"),
            ));
        }
    }

    let body = Task {
        description: args.description.clone().unwrap_or_default(),
        parent_epic,
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        claim: None,
    };
    let mut builder = builder_with_common(
        RecordKind::Task,
        &args.title,
        actor,
        args.priority.map(super::priority_to_core),
        args.scope.as_deref(),
    )
    .task(body);
    if let Some(owner) = &args.owner {
        let identity =
            Identity::new(owner.clone()).map_err(|e| CliError::user(CMD_TASK, e.to_string()))?;
        builder = builder.owner(identity);
    }
    builder = apply_labels(builder, &args.labels, CMD_TASK)?;
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_TASK, e.to_string()))?;
    apply_label_envelope(&mut record, &args.labels, CMD_TASK)?;
    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Created(RecordOutcome {
        command: CMD_TASK,
        record,
    }))
}

/// `firetrail subtask create`
pub fn subtask(args: &CreateSubtaskArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_SUBTASK, global.workspace.as_deref())?;
    let actor = ctx.actor()?;

    let parent = ctx.resolve_id(&args.parent)?;
    if parent.kind() != RecordKind::Task {
        return Err(CliError::user(
            CMD_SUBTASK,
            format!("--parent {parent} is not a task"),
        ));
    }

    let body = Subtask {
        description: args.description.clone().unwrap_or_default(),
        parent_task: parent,
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        claim: None,
    };
    let mut builder = builder_with_common(
        RecordKind::Subtask,
        &args.title,
        actor,
        args.priority.map(super::priority_to_core),
        args.scope.as_deref(),
    )
    .subtask(body);
    if let Some(owner) = &args.owner {
        let identity =
            Identity::new(owner.clone()).map_err(|e| CliError::user(CMD_SUBTASK, e.to_string()))?;
        builder = builder.owner(identity);
    }
    builder = apply_labels(builder, &args.labels, CMD_SUBTASK)?;
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_SUBTASK, e.to_string()))?;
    apply_label_envelope(&mut record, &args.labels, CMD_SUBTASK)?;
    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Created(RecordOutcome {
        command: CMD_SUBTASK,
        record,
    }))
}

/// `firetrail bug create`
pub fn bug(args: &CreateBugArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_BUG, global.workspace.as_deref())?;
    let actor = ctx.actor()?;

    let body = Bug {
        description: args.description.clone().unwrap_or_default(),
        service: args.service.clone(),
        severity: args.severity.clone(),
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        claim: None,
    };
    let mut builder = builder_with_common(
        RecordKind::Bug,
        &args.title,
        actor,
        args.priority.map(super::priority_to_core),
        args.scope.as_deref(),
    )
    .bug(body);
    builder = apply_labels(builder, &args.labels, CMD_BUG)?;
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_BUG, e.to_string()))?;
    apply_label_envelope(&mut record, &args.labels, CMD_BUG)?;
    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Created(RecordOutcome {
        command: CMD_BUG,
        record,
    }))
}

fn builder_with_common(
    kind: RecordKind,
    title: &str,
    created_by: Identity,
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

/// The `RecordBuilder` does not expose a label setter directly; we pre-validate
/// the args here and then re-apply on the constructed envelope (a separate
/// pass which re-hashes via [`WorkCtx::save_record`]).
fn apply_labels(
    builder: RecordBuilder,
    labels: &[String],
    command: &str,
) -> Result<RecordBuilder, CliError> {
    for raw in labels {
        // Validate each label up-front so we fail before constructing the
        // record.
        let _ = parse_label_pair(command, raw)?;
    }
    Ok(builder)
}

fn apply_label_envelope(
    record: &mut ft_core::Record,
    labels: &[String],
    command: &str,
) -> Result<(), CliError> {
    for raw in labels {
        let (key, value) = parse_label_pair(command, raw)?;
        record.envelope.labels.push(Label { key, value });
    }
    Ok(())
}
