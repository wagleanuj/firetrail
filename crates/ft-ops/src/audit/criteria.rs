//! Acceptance-criteria CRUD ops over ticket records.
//!
//! Mirrors `ft_cli::commands::criteria`. Each write op emits a
//! [`crate::Event::TicketUpdated`] so the GUI's ticket views can react to
//! AC changes without polling.

use chrono::Utc;
use ft_core::{AcStatus, AcceptanceCriterion, Record, RecordBody};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::tickets::ctx_for_criteria;
use crate::workspace::Workspace;

/// Input for [`criteria_add`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaAddInput {
    /// Record id (Task / Subtask / Bug).
    pub id: String,
    /// Criterion text (non-empty after trim).
    pub text: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of any write-side criteria op — the updated record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CriteriaWriteOutput {
    /// The updated record.
    pub record: Record,
}

/// Input for [`criteria_list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaListInput {
    /// Record id.
    pub id: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// One AC row in [`CriteriaListOutput`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaListRow {
    /// 1-based position in the list.
    pub index: usize,
    /// Local id (e.g. `ac-01`).
    pub id: String,
    /// Criterion text.
    pub text: String,
    /// Whether the AC is checked.
    pub checked: bool,
    /// Attached evidence URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_url: Option<String>,
}

/// Output of [`criteria_list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaListOutput {
    /// Canonical record id.
    pub record_id: String,
    /// AC rows.
    pub items: Vec<CriteriaListRow>,
}

/// Input for [`criteria_check`] / [`criteria_uncheck`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaToggleInput {
    /// Record id.
    pub id: String,
    /// AC id (`ac-01`) or 1-based index.
    pub which: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`criteria_evidence`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaEvidenceInput {
    /// Record id.
    pub id: String,
    /// AC id or 1-based index.
    pub which: String,
    /// Evidence URL (non-empty after trim).
    pub url: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// `criteria add` op.
#[allow(clippy::needless_pass_by_value)]
pub fn criteria_add(
    ws: &Workspace,
    identity: &Identity,
    input: CriteriaAddInput,
    events: &EventBus,
) -> Result<CriteriaWriteOutput, OpsError> {
    let mut ctx = ctx_for_criteria(ws, identity, "criteria add")?;
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;

    let text = input.text.trim();
    if text.is_empty() {
        return Err(OpsError::validation("text", "criterion text cannot be empty"));
    }
    let acs = criteria_mut(&mut record.body).ok_or_else(|| {
        OpsError::validation("id", "this record kind does not support acceptance criteria")
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
    ctx.save_record(&mut record)?;
    emit_updated(events, input.request_id.as_deref(), &id);
    Ok(CriteriaWriteOutput { record })
}

/// `criteria list` op.
#[allow(clippy::needless_pass_by_value)]
pub fn criteria_list(
    ws: &Workspace,
    identity: &Identity,
    input: CriteriaListInput,
    _events: &EventBus,
) -> Result<CriteriaListOutput, OpsError> {
    let ctx = ctx_for_criteria(ws, identity, "criteria list")?;
    let id = ctx.resolve_id(&input.id)?;
    let record = ctx.read_record(&id)?;
    let acs = criteria_ref(&record.body).unwrap_or(&[]);
    let items: Vec<CriteriaListRow> = acs
        .iter()
        .enumerate()
        .map(|(idx, ac)| CriteriaListRow {
            index: idx + 1,
            id: ac.id.clone(),
            text: ac.text.clone(),
            checked: matches!(ac.status, AcStatus::Checked),
            evidence_url: ac.evidence_url.clone(),
        })
        .collect();
    Ok(CriteriaListOutput {
        record_id: id.as_str().to_string(),
        items,
    })
}

/// `criteria check` op.
pub fn criteria_check(
    ws: &Workspace,
    identity: &Identity,
    input: CriteriaToggleInput,
    events: &EventBus,
) -> Result<CriteriaWriteOutput, OpsError> {
    toggle(ws, identity, input, events, true, "criteria check")
}

/// `criteria uncheck` op.
pub fn criteria_uncheck(
    ws: &Workspace,
    identity: &Identity,
    input: CriteriaToggleInput,
    events: &EventBus,
) -> Result<CriteriaWriteOutput, OpsError> {
    toggle(ws, identity, input, events, false, "criteria uncheck")
}

#[allow(clippy::needless_pass_by_value)]
fn toggle(
    ws: &Workspace,
    identity: &Identity,
    input: CriteriaToggleInput,
    events: &EventBus,
    checked: bool,
    op: &'static str,
) -> Result<CriteriaWriteOutput, OpsError> {
    let mut ctx = ctx_for_criteria(ws, identity, op)?;
    let id = ctx.resolve_id(&input.id)?;
    let actor = ctx.actor.clone();
    let mut record = ctx.read_record(&id)?;
    let now = Utc::now();

    {
        let acs = criteria_mut(&mut record.body).ok_or_else(|| {
            OpsError::validation("id", "this record kind does not support acceptance criteria")
        })?;
        let ac = find_criterion_mut(acs, &input.which)?;
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
    emit_updated(events, input.request_id.as_deref(), &id);
    Ok(CriteriaWriteOutput { record })
}

/// `criteria evidence` op.
#[allow(clippy::needless_pass_by_value)]
pub fn criteria_evidence(
    ws: &Workspace,
    identity: &Identity,
    input: CriteriaEvidenceInput,
    events: &EventBus,
) -> Result<CriteriaWriteOutput, OpsError> {
    let mut ctx = ctx_for_criteria(ws, identity, "criteria evidence")?;
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let now = Utc::now();

    {
        let acs = criteria_mut(&mut record.body).ok_or_else(|| {
            OpsError::validation("id", "this record kind does not support acceptance criteria")
        })?;
        let ac = find_criterion_mut(acs, &input.which)?;
        let url = input.url.trim();
        if url.is_empty() {
            return Err(OpsError::validation("url", "evidence URL cannot be empty"));
        }
        ac.evidence_url = Some(url.to_string());
        ac.updated_at = now;
    }
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;
    emit_updated(events, input.request_id.as_deref(), &id);
    Ok(CriteriaWriteOutput { record })
}

fn emit_updated(bus: &EventBus, request_id: Option<&str>, id: &ft_core::RecordId) {
    let event = Event::TicketUpdated {
        id: id.as_str().to_string(),
    };
    if let Some(rid) = request_id {
        bus.emit_with_request(rid.to_string(), event);
    } else {
        bus.emit(event);
    }
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
) -> Result<&'a mut AcceptanceCriterion, OpsError> {
    if let Ok(idx) = which.parse::<usize>() {
        if idx == 0 || idx > acs.len() {
            return Err(OpsError::validation(
                "which",
                format!("index {idx} is out of range (have {})", acs.len()),
            ));
        }
        return Ok(&mut acs[idx - 1]);
    }
    if let Some(ac) = acs.iter_mut().find(|a| a.id == which) {
        return Ok(ac);
    }
    Err(OpsError::not_found(
        "acceptance criterion",
        which.to_string(),
    ))
}
