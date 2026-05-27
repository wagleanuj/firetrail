//! `firetrail criteria …` — acceptance-criteria management.

use chrono::Utc;
use ft_core::{AcStatus, AcceptanceCriterion, RecordBody};
use serde::Serialize;

use crate::cli::{
    CriteriaAddArgs, CriteriaEvidenceArgs, CriteriaListArgs, CriteriaToggleArgs, GlobalOpts,
};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND_ADD: &str = "criteria add";
const COMMAND_LIST: &str = "criteria list";
const COMMAND_CHECK: &str = "criteria check";
const COMMAND_UNCHECK: &str = "criteria uncheck";
const COMMAND_EVIDENCE: &str = "criteria evidence";

/// `firetrail criteria add`
pub fn add(args: &CriteriaAddArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_ADD, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let actor = ctx.actor()?;
    let mut record = ctx.read_record(&id)?;

    let text = args.text.trim();
    if text.is_empty() {
        return Err(CliError::user(
            COMMAND_ADD,
            "criterion text cannot be empty",
        ));
    }

    let acs = criteria_mut(&mut record.body).ok_or_else(|| {
        CliError::user(
            COMMAND_ADD,
            "this record kind does not support acceptance criteria",
        )
    })?;
    let next_index = acs.len() + 1;
    let new_id = format!("ac-{next_index:02}");
    let now = Utc::now();
    acs.push(AcceptanceCriterion {
        id: new_id,
        text: text.to_string(),
        status: AcStatus::Unchecked,
        evidence_url: None,
        checked_by: None,
        checked_at: None,
        created_at: now,
        updated_at: now,
        proposed: false,
    });
    record.envelope.updated_at = now;
    let _ = actor; // currently unused, would feed proposed-by in M2

    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Updated(RecordOutcome {
        command: COMMAND_ADD,
        record,
    }))
}

/// `firetrail criteria list`
pub fn list(args: &CriteriaListArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND_LIST, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let record = ctx.read_record(&id)?;
    let acs = criteria_ref(&record.body).unwrap_or(&[]);
    let items: Vec<CriteriaListItem> = acs
        .iter()
        .enumerate()
        .map(|(idx, ac)| CriteriaListItem {
            index: idx + 1,
            id: ac.id.clone(),
            text: ac.text.clone(),
            checked: matches!(ac.status, AcStatus::Checked),
            evidence_url: ac.evidence_url.clone(),
        })
        .collect();
    Ok(CommandOutcome::CriteriaList(CriteriaListOutcome {
        record_id: id.as_str().to_string(),
        items,
    }))
}

/// `firetrail criteria check`
pub fn check(args: &CriteriaToggleArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    toggle(args, global, COMMAND_CHECK, true)
}

/// `firetrail criteria uncheck`
pub fn uncheck(args: &CriteriaToggleArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    toggle(args, global, COMMAND_UNCHECK, false)
}

fn toggle(
    args: &CriteriaToggleArgs,
    global: &GlobalOpts,
    command: &'static str,
    checked: bool,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(command, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let actor = ctx.actor()?;
    let mut record = ctx.read_record(&id)?;
    let now = Utc::now();

    {
        let acs = criteria_mut(&mut record.body).ok_or_else(|| {
            CliError::user(
                command,
                "this record kind does not support acceptance criteria",
            )
        })?;
        let ac = find_criterion_mut(acs, &args.which, command)?;
        ac.status = if checked {
            AcStatus::Checked
        } else {
            AcStatus::Unchecked
        };
        if checked {
            ac.checked_by = Some(actor);
            ac.checked_at = Some(now);
        } else {
            ac.checked_by = None;
            ac.checked_at = None;
        }
        ac.updated_at = now;
    }
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;

    Ok(CommandOutcome::Updated(RecordOutcome { command, record }))
}

/// `firetrail criteria evidence`
pub fn evidence(
    args: &CriteriaEvidenceArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_EVIDENCE, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;
    let now = Utc::now();

    {
        let acs = criteria_mut(&mut record.body).ok_or_else(|| {
            CliError::user(
                COMMAND_EVIDENCE,
                "this record kind does not support acceptance criteria",
            )
        })?;
        let ac = find_criterion_mut(acs, &args.which, COMMAND_EVIDENCE)?;
        let url = args.url.trim();
        if url.is_empty() {
            return Err(CliError::user(COMMAND_EVIDENCE, "--url cannot be empty"));
        }
        ac.evidence_url = Some(url.to_string());
        ac.updated_at = now;
    }
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;
    Ok(CommandOutcome::Updated(RecordOutcome {
        command: COMMAND_EVIDENCE,
        record,
    }))
}

fn criteria_ref(body: &RecordBody) -> Option<&[AcceptanceCriterion]> {
    match body {
        RecordBody::Task(t) => Some(&t.acceptance_criteria),
        RecordBody::Subtask(s) => Some(&s.acceptance_criteria),
        RecordBody::Bug(b) => Some(&b.acceptance_criteria),
        _ => None,
    }
}

fn criteria_mut(body: &mut RecordBody) -> Option<&mut Vec<AcceptanceCriterion>> {
    match body {
        RecordBody::Task(t) => Some(&mut t.acceptance_criteria),
        RecordBody::Subtask(s) => Some(&mut s.acceptance_criteria),
        RecordBody::Bug(b) => Some(&mut b.acceptance_criteria),
        _ => None,
    }
}

fn find_criterion_mut<'a>(
    acs: &'a mut [AcceptanceCriterion],
    which: &str,
    command: &str,
) -> Result<&'a mut AcceptanceCriterion, CliError> {
    // First: try as 1-based index.
    if let Ok(idx) = which.parse::<usize>() {
        if idx == 0 || idx > acs.len() {
            return Err(CliError::user(
                command,
                format!("index {idx} is out of range (have {})", acs.len()),
            ));
        }
        return Ok(&mut acs[idx - 1]);
    }
    // Otherwise: match by AC id.
    if let Some(ac) = acs.iter_mut().find(|a| a.id == which) {
        return Ok(ac);
    }
    Err(CliError::NotFound {
        command: command.into(),
        what: format!("acceptance criterion `{which}`"),
    })
}

#[derive(Debug, Clone, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CriteriaListItem {
    pub index: usize,
    pub id: String,
    pub text: String,
    pub checked: bool,
    pub evidence_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CriteriaListOutcome {
    pub record_id: String,
    pub items: Vec<CriteriaListItem>,
}

impl CriteriaListOutcome {
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!("# Acceptance criteria for `{}`\n\n", self.record_id);
        if self.items.is_empty() {
            s.push_str("_no criteria_\n");
            return s;
        }
        for it in &self.items {
            let mark = if it.checked { "x" } else { " " };
            let _ = writeln!(s, "- [{mark}] **{}** {}", it.id, it.text);
            if let Some(url) = &it.evidence_url {
                let _ = writeln!(s, "    evidence: {url}");
            }
        }
        s
    }

    pub fn quiet_line(&self) -> String {
        let done = self.items.iter().filter(|i| i.checked).count();
        format!("{}: {done}/{}", self.record_id, self.items.len())
    }
}
