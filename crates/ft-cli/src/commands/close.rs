//! `firetrail close <id>` / `firetrail reopen <id>`.
//!
//! `close` validates that every acceptance criterion is checked. `--force`
//! overrides that gate and requires `--reason` (enforced by clap). The reason
//! is recorded as a `force_close_reason` label on the envelope (the canonical
//! history-entry path lands with `ft-history` in M2).

use chrono::Utc;
use ft_core::{AcStatus, AcceptanceCriterion, Label, RecordBody, Status};

use crate::cli::{CloseArgs, GlobalOpts, ReopenArgs};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND_CLOSE: &str = "close";
const COMMAND_REOPEN: &str = "reopen";

/// `firetrail close`
pub fn close(args: &CloseArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_CLOSE, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;

    if record.envelope.status == Status::Closed {
        return Err(CliError::user(COMMAND_CLOSE, "record is already closed"));
    }

    let incomplete = unchecked_criteria(&record.body);
    if !incomplete.is_empty() && !args.force {
        return Err(CliError::UserError {
            command: COMMAND_CLOSE.into(),
            message: format!("{} acceptance criteria are incomplete", incomplete.len()),
            details: serde_json::json!({
                "incomplete": incomplete
                    .iter()
                    .map(|a| serde_json::json!({ "id": a.id, "text": a.text }))
                    .collect::<Vec<_>>()
            }),
        });
    }

    if args.force {
        let reason = args
            .reason
            .clone()
            .ok_or_else(|| CliError::user(COMMAND_CLOSE, "--force requires --reason"))?;
        record.envelope.labels.push(Label {
            key: "force_close_reason".into(),
            value: reason,
        });
    }

    record.envelope.status = Status::Closed;
    let now = Utc::now();
    record.envelope.closed_at = Some(now);
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;

    Ok(CommandOutcome::Closed(
        RecordOutcome::new(COMMAND_CLOSE, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail reopen`
pub fn reopen(args: &ReopenArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_REOPEN, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;

    if record.envelope.status != Status::Closed && record.envelope.status != Status::Deferred {
        return Err(CliError::user(
            COMMAND_REOPEN,
            "record is not in a closed/deferred state",
        ));
    }

    record.envelope.status = Status::Open;
    record.envelope.closed_at = None;
    record.envelope.updated_at = Utc::now();
    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(COMMAND_REOPEN, record).with_warnings(ctx.warnings.clone()),
    ))
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
