//! `firetrail {incident,finding,runbook,decision,gotcha,memory} create` and
//! `firetrail capture` — write a new memory record with a Create-kind
//! history entry.
//!
//! Every create routes through [`ft_storage::write_with_history`] so the
//! per-record chain is bootstrapped with a genesis entry. This matches the
//! M2 contract that every on-disk record carries at least a Create entry.

use std::io::Read as _;

use chrono::Utc;
use ft_core::{
    Decision, Finding, Gotcha, Identity, Incident, Memory, Origin, RecordBuilder, RecordKind,
    Runbook, Severity,
};
use ft_history::{HistoryDraft, HistoryEntryKind};
use ft_storage::write_with_history;

use crate::cli::{
    CaptureArgs, CreateDecisionArgs, CreateFindingArgs, CreateGotchaArgs, CreateIncidentArgs,
    CreateMemoryArgs, CreateRunbookArgs, GlobalOpts, MemoryKindArg,
};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_INCIDENT: &str = "incident create";
const CMD_FINDING: &str = "finding create";
const CMD_RUNBOOK: &str = "runbook create";
const CMD_DECISION: &str = "decision create";
const CMD_GOTCHA: &str = "gotcha create";
const CMD_MEMORY: &str = "memory create";
const CMD_CAPTURE: &str = "capture";

/// `firetrail incident create`
pub fn incident(
    args: &CreateIncidentArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_INCIDENT, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let started_at = match &args.started_at {
        Some(s) => parse_rfc3339(CMD_INCIDENT, s)?,
        None => Utc::now(),
    };
    let body = Incident {
        summary: args.summary.clone(),
        severity: args
            .severity
            .map_or(Severity::Sev3, super::super::cli::SeverityArg::to_core),
        started_at,
        resolved_at: None,
        services_affected: split_csv(args.services.as_deref()),
        root_cause: None,
        findings: Vec::new(),
        runbooks_invoked: Vec::new(),
        risk_class: args
            .risk_class
            .map(super::super::cli::RiskClassArg::to_core),
        trust: ft_core::TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Incident, &args.summary, actor.clone())
        .incident(body)
        .origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_INCIDENT, e.to_string()))?;
    write_with_create(
        &mut ctx,
        &mut record,
        &actor,
        CMD_INCIDENT,
        "incident created",
    )?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_INCIDENT, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail finding create`
pub fn finding(args: &CreateFindingArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_FINDING, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let incident_id = args
        .incident
        .as_deref()
        .map(|raw| ctx.resolve_id(raw))
        .transpose()?;
    if let Some(p) = &incident_id {
        if p.kind() != RecordKind::Incident {
            return Err(CliError::user(
                CMD_FINDING,
                format!("--incident {p} is not an incident"),
            ));
        }
    }
    let body = Finding {
        summary: args.summary.clone(),
        details: args.details.clone().unwrap_or_default(),
        incident: incident_id,
        risk_class: args
            .risk_class
            .map(super::super::cli::RiskClassArg::to_core),
        affected_paths: split_csv(args.affected.as_deref()),
        superseded_by: None,
        trust: ft_core::TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Finding, &args.summary, actor.clone())
        .finding(body)
        .origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_FINDING, e.to_string()))?;
    write_with_create(
        &mut ctx,
        &mut record,
        &actor,
        CMD_FINDING,
        "finding created",
    )?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_FINDING, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail runbook create`
pub fn runbook(args: &CreateRunbookArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_RUNBOOK, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let body = Runbook {
        title: args.title.clone(),
        summary: args.summary.clone(),
        steps: Vec::new(),
        applies_to: split_csv(args.applies_to.as_deref()),
        risk_class: args
            .risk_class
            .map(super::super::cli::RiskClassArg::to_core),
        trust: ft_core::TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Runbook, &args.title, actor.clone())
        .runbook(body)
        .origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_RUNBOOK, e.to_string()))?;
    write_with_create(
        &mut ctx,
        &mut record,
        &actor,
        CMD_RUNBOOK,
        "runbook created",
    )?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_RUNBOOK, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail decision create`
pub fn decision(
    args: &CreateDecisionArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_DECISION, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let body = Decision {
        title: args.title.clone(),
        context: args.context.clone(),
        decision: args.decision.clone(),
        consequences: args.consequences.clone().unwrap_or_default(),
        alternatives_considered: Vec::new(),
        status: ft_core::DecisionStatus::default(),
        superseded_by: None,
        risk_class: args
            .risk_class
            .map(super::super::cli::RiskClassArg::to_core),
        trust: ft_core::TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Decision, &args.title, actor.clone())
        .decision(body)
        .origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_DECISION, e.to_string()))?;
    write_with_create(
        &mut ctx,
        &mut record,
        &actor,
        CMD_DECISION,
        "decision created",
    )?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_DECISION, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail gotcha create`
pub fn gotcha(args: &CreateGotchaArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_GOTCHA, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let body = Gotcha {
        summary: args.summary.clone(),
        details: args.details.clone().unwrap_or_default(),
        affected_paths: split_csv(args.affected.as_deref()),
        risk_class: args
            .risk_class
            .map(super::super::cli::RiskClassArg::to_core),
        trust: ft_core::TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Gotcha, &args.summary, actor.clone())
        .gotcha(body)
        .origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_GOTCHA, e.to_string()))?;
    write_with_create(&mut ctx, &mut record, &actor, CMD_GOTCHA, "gotcha created")?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_GOTCHA, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory create`
pub fn memory(args: &CreateMemoryArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_MEMORY, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let body = Memory {
        title: args.title.clone(),
        body: args.body.clone(),
        tags: split_csv(args.tags.as_deref()),
        related: Vec::new(),
        risk_class: args
            .risk_class
            .map(super::super::cli::RiskClassArg::to_core),
        trust: ft_core::TrustState::Draft,
    };
    let mut builder = RecordBuilder::new(RecordKind::Memory, &args.title, actor.clone())
        .memory(body)
        .origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_MEMORY, e.to_string()))?;
    write_with_create(&mut ctx, &mut record, &actor, CMD_MEMORY, "memory created")?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_MEMORY, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail capture` — opportunistic memory capture. Reads `--body` or
/// stdin into a memory-kind record (defaulting to generic `memory`).
pub fn capture(args: &CaptureArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_CAPTURE, global.workspace.as_deref())?;
    let actor = ctx.actor()?;

    let body_text = if let Some(s) = &args.body {
        s.clone()
    } else {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::user(CMD_CAPTURE, format!("reading stdin: {e}")))?;
        buf.trim().to_string()
    };
    if body_text.is_empty() {
        return Err(CliError::user(
            CMD_CAPTURE,
            "body is empty (supply --body or pipe content into stdin)",
        ));
    }

    let kind = args.kind.to_core();
    let tags = split_csv(args.tags.as_deref());
    let risk = args
        .risk_class
        .map(super::super::cli::RiskClassArg::to_core);

    let mut builder = RecordBuilder::new(kind, &args.title, actor.clone()).origin(Origin::Human);
    if let Some(s) = &args.scope {
        builder = builder.owning_scope(s.clone());
    }
    builder = match args.kind {
        MemoryKindArg::Memory => builder.memory(Memory {
            title: args.title.clone(),
            body: body_text.clone(),
            tags: tags.clone(),
            related: Vec::new(),
            risk_class: risk,
            trust: ft_core::TrustState::Draft,
        }),
        MemoryKindArg::Gotcha => builder.gotcha(Gotcha {
            summary: args.title.clone(),
            details: body_text.clone(),
            affected_paths: Vec::new(),
            risk_class: risk,
            trust: ft_core::TrustState::Draft,
        }),
        MemoryKindArg::Finding => builder.finding(Finding {
            summary: args.title.clone(),
            details: body_text.clone(),
            incident: None,
            risk_class: risk,
            affected_paths: Vec::new(),
            superseded_by: None,
            trust: ft_core::TrustState::Draft,
        }),
        MemoryKindArg::Incident => builder.incident(Incident {
            summary: args.title.clone(),
            severity: Severity::Sev3,
            started_at: Utc::now(),
            resolved_at: None,
            services_affected: Vec::new(),
            root_cause: Some(body_text.clone()),
            findings: Vec::new(),
            runbooks_invoked: Vec::new(),
            risk_class: risk,
            trust: ft_core::TrustState::Draft,
        }),
        MemoryKindArg::Runbook => builder.runbook(Runbook {
            title: args.title.clone(),
            summary: body_text.clone(),
            steps: Vec::new(),
            applies_to: Vec::new(),
            risk_class: risk,
            trust: ft_core::TrustState::Draft,
        }),
        MemoryKindArg::Decision => builder.decision(Decision {
            title: args.title.clone(),
            context: String::new(),
            decision: body_text.clone(),
            consequences: String::new(),
            alternatives_considered: Vec::new(),
            status: ft_core::DecisionStatus::default(),
            superseded_by: None,
            risk_class: risk,
            trust: ft_core::TrustState::Draft,
        }),
    };

    let mut record = builder
        .build()
        .map_err(|e| CliError::user(CMD_CAPTURE, e.to_string()))?;
    write_with_create(&mut ctx, &mut record, &actor, CMD_CAPTURE, "capture")?;
    Ok(CommandOutcome::Created(
        RecordOutcome::new(CMD_CAPTURE, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// Shared post-build write path: append a Create history entry and persist.
/// Also refreshes the index from the resulting on-disk path.
pub(crate) fn write_with_create(
    ctx: &mut WorkCtx,
    record: &mut ft_core::Record,
    actor: &Identity,
    command: &'static str,
    summary: &str,
) -> Result<(), CliError> {
    let draft = HistoryDraft {
        merged_via_pr: None,
        timestamp: Utc::now(),
        primary_actor: actor.clone(),
        contributors: Vec::new(),
        ops_summary: vec![summary.to_string()],
        ops_count: 1,
        kind: HistoryEntryKind::Create,
    };
    let path = write_with_history(&ctx.storage, record, draft)
        .map_err(|e| CliError::internal(command, format!("write: {e}")))?;
    ctx.index
        .refresh(&ctx.storage, std::slice::from_ref(&path), &[])
        .map_err(|e| CliError::internal(command, format!("refresh index: {e}")))?;
    Ok(())
}

/// Parse an RFC3339 timestamp or surface a user error tied to `command`.
fn parse_rfc3339(command: &'static str, raw: &str) -> Result<chrono::DateTime<Utc>, CliError> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| CliError::user(command, format!("invalid timestamp `{raw}`: {e}")))
}

/// Split a comma-separated string into trimmed, non-empty entries.
fn split_csv(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}
