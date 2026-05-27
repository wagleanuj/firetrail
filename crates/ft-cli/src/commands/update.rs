//! `firetrail update <id>` — mutate envelope fields on an existing record.
//!
//! Only fields touched on the command line are changed; everything else is
//! preserved verbatim. `state_hash` is recomputed by [`WorkCtx::save_record`].

use chrono::Utc;
use ft_core::{Identity, Status};

use crate::cli::{GlobalOpts, UpdateArgs};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "update";

/// Entry point.
pub fn run(args: &UpdateArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;

    let mut touched = false;
    if let Some(t) = &args.title {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            return Err(CliError::user(COMMAND, "title cannot be empty"));
        }
        record.envelope.title = trimmed.to_string();
        touched = true;
    }
    if let Some(s) = args.status {
        let new = s.to_core();
        if new == Status::Closed && record.envelope.status != Status::Closed {
            record.envelope.closed_at = Some(Utc::now());
        }
        if new != Status::Closed {
            record.envelope.closed_at = None;
        }
        record.envelope.status = new;
        touched = true;
    }
    if let Some(p) = args.priority {
        record.envelope.priority = p.to_core();
        touched = true;
    }
    if let Some(owner) = &args.owner {
        let trimmed = owner.trim();
        if trimmed.is_empty() {
            record.envelope.owner = None;
        } else {
            let identity = Identity::new(trimmed)
                .map_err(|e| CliError::user(COMMAND, format!("invalid owner: {e}")))?;
            record.envelope.owner = Some(identity);
        }
        touched = true;
    }

    if !touched {
        return Err(CliError::user(
            COMMAND,
            "no fields to update; supply at least one of --title --status --priority --owner",
        ));
    }

    record.envelope.updated_at = Utc::now();
    ctx.save_record(&mut record)?;

    Ok(CommandOutcome::Updated(
        RecordOutcome::new(COMMAND, record).with_warnings(ctx.warnings.clone()),
    ))
}
