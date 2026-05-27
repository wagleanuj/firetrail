//! `firetrail memory {review,promote,deprecate,archive,supersede,merge,redact}`.
//!
//! Every command follows the same three-step write path:
//!
//! 1. Resolve the actor and read the record from storage.
//! 2. Source the `prior_reviewers` list from the record's existing
//!    `history[]` (any prior [`HistoryEntryKind::TrustTransition`] entries
//!    contribute their `primary_actor`).
//! 3. Validate the transition with [`ft_trust::validate_transition`], apply
//!    it via [`ft_trust::apply_transition`], and then commit the change to
//!    disk in a single [`ft_storage::write_with_history`] call that appends
//!    a [`HistoryEntryKind::TrustTransition`] entry. Validation failures
//!    surface as `CliError::user` so the on-disk record is unchanged.
//!
//! ADR-0013 enforcement (agent-cannot-promote, distinct reviewers, evidence
//! for high-stakes) lives entirely in `ft-trust`; this module is a thin
//! adapter that translates CLI flags into the `TrustTransition` shape the
//! state machine wants.

use chrono::Utc;
use ft_core::{Evidence, Identity, Origin, Record, RecordBody, TrustState};
use ft_history::{HistoryDraft, HistoryEntryKind};
use ft_storage::write_with_history;
use ft_trust::{MemoryBody, TrustTransition, apply_transition, validate_transition};

use crate::cli::{
    GlobalOpts, TrustMergeArgs, TrustPromoteArgs, TrustReasonArgs, TrustReviewArgs,
    TrustSimpleArgs, TrustSupersedeArgs,
};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_REVIEW: &str = "memory review";
const CMD_PROMOTE: &str = "memory promote";
const CMD_DEPRECATE: &str = "memory deprecate";
const CMD_ARCHIVE: &str = "memory archive";
const CMD_SUPERSEDE: &str = "memory supersede";
const CMD_MERGE: &str = "memory merge";
const CMD_REDACT: &str = "memory redact";

/// `firetrail memory review`
pub fn review(args: &TrustReviewArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_REVIEW, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;
    let evidence = build_evidence_url(&actor, args.evidence_url.as_deref(), None);
    let transition = build_transition(
        &record,
        TrustState::Reviewed,
        actor.clone(),
        args.reason.clone(),
        evidence,
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_REVIEW)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_REVIEW, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory promote`
pub fn promote(args: &TrustPromoteArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_PROMOTE, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;
    let evidence_kind = args
        .evidence_type
        .map(super::super::cli::EvidenceKindArg::to_core);
    let evidence = build_evidence_url(&actor, args.evidence_url.as_deref(), evidence_kind);
    let transition = build_transition(
        &record,
        TrustState::Verified,
        actor.clone(),
        args.reason.clone(),
        evidence,
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_PROMOTE)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_PROMOTE, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory deprecate`
pub fn deprecate(args: &TrustReasonArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_DEPRECATE, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Deprecated,
        actor.clone(),
        Some(args.reason.clone()),
        Vec::new(),
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_DEPRECATE)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_DEPRECATE, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory archive`
pub fn archive(args: &TrustSimpleArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_ARCHIVE, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Archived,
        actor.clone(),
        None,
        Vec::new(),
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_ARCHIVE)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_ARCHIVE, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory supersede`
pub fn supersede(
    args: &TrustSupersedeArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_SUPERSEDE, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let successor = ctx.resolve_id(&args.successor)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Superseded,
        actor.clone(),
        args.reason.clone(),
        Vec::new(),
        Some(successor),
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_SUPERSEDE)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_SUPERSEDE, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory merge` — supersede every "other" id with `canonical`.
pub fn merge(args: &TrustMergeArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_MERGE, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let canonical = ctx.resolve_id(&args.canonical)?;
    if args.others.is_empty() {
        return Err(CliError::user(
            CMD_MERGE,
            "merge requires at least one other record id",
        ));
    }
    let mut last_record: Option<Record> = None;
    for raw in &args.others {
        let other_id = ctx.resolve_id(raw)?;
        if other_id == canonical {
            return Err(CliError::user(
                CMD_MERGE,
                format!("cannot merge {other_id} into itself"),
            ));
        }
        let mut record = ctx.read_record(&other_id)?;
        let transition = build_transition(
            &record,
            TrustState::Superseded,
            actor.clone(),
            args.reason
                .clone()
                .or_else(|| Some(format!("merged into {canonical}"))),
            Vec::new(),
            Some(canonical.clone()),
        )?;
        apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_MERGE)?;
        last_record = Some(record);
    }
    let final_record = last_record.expect("validated non-empty above");
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_MERGE, final_record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail memory redact`
pub fn redact(args: &TrustReasonArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_REDACT, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;
    let transition = build_transition(
        &record,
        TrustState::Redacted,
        actor.clone(),
        Some(args.reason.clone()),
        Vec::new(),
        None,
    )?;
    apply_and_persist(&mut ctx, &mut record, &actor, &transition, CMD_REDACT)?;
    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD_REDACT, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// Walk `record.envelope.history` and return every distinct actor that
/// participated in a prior `TrustTransition` entry. The list is the
/// `prior_reviewers` input the trust state machine expects so it can enforce
/// reviewer-distinctness and the two-reviewer requirement for Verified.
fn prior_reviewers(record: &Record) -> Vec<Identity> {
    let mut out: Vec<Identity> = Vec::new();
    for entry in &record.envelope.history {
        let head = entry.ops_summary.first().map_or("", String::as_str);
        let tag = head.split_once(':').map_or("", |(t, _)| t.trim());
        if HistoryEntryKind::from_tag(tag) == Some(HistoryEntryKind::TrustTransition)
            && !out.contains(&entry.primary_actor)
        {
            out.push(entry.primary_actor.clone());
        }
    }
    out
}

/// Helper: read the current trust state and risk classification from the
/// body, surfacing a user error if the record is not memory-kind.
fn current_trust(
    record: &mut Record,
    command: &'static str,
) -> Result<(TrustState, Option<ft_core::RiskClass>), CliError> {
    let view = MemoryBody::from_record_body(&mut record.body)
        .map_err(|e| CliError::user(command, format!("not a memory record: {e}")))?;
    Ok((view.trust(), view.risk_class()))
}

/// Construct a `TrustTransition` populated from the CLI flags and the
/// record's current state. Pure: no I/O, no validation here — that lives in
/// `apply_and_persist`.
fn build_transition(
    record: &Record,
    to: TrustState,
    reviewer: Identity,
    reason: Option<String>,
    evidence: Vec<Evidence>,
    successor: Option<ft_core::RecordId>,
) -> Result<TrustTransition, CliError> {
    let mut mutable = record.clone();
    let (from, _risk) = current_trust(&mut mutable, "memory trust")?;
    Ok(TrustTransition {
        from,
        to,
        reviewer,
        origin: Origin::Human,
        reason,
        evidence,
        successor,
        occurred_at: Utc::now(),
    })
}

/// Validate, apply, then `write_with_history` so the chain entry is
/// appended atomically with the body mutation. Failure at validate-time
/// leaves the on-disk record untouched.
fn apply_and_persist(
    ctx: &mut WorkCtx,
    record: &mut Record,
    actor: &Identity,
    transition: &TrustTransition,
    command: &'static str,
) -> Result<(), CliError> {
    // Snapshot prior reviewers from existing history, then validate.
    let prior = prior_reviewers(record);
    let (current_state, risk) = current_trust(record, command)?;
    if transition.from != current_state {
        return Err(CliError::user(
            command,
            format!(
                "record is in {:?}, refusing transition that expects {:?}",
                current_state, transition.from
            ),
        ));
    }
    let author = record.envelope.created_by.clone();
    validate_transition(current_state, risk, transition, &prior, &author)
        .map_err(|e| CliError::user(command, e.to_string()))?;

    // Apply to the body view (scoped so the borrow drops before we re-touch the record).
    let applied = {
        let mut view = MemoryBody::from_record_body(&mut record.body)
            .map_err(|e| CliError::user(command, format!("not a memory record: {e}")))?;
        apply_transition(&mut view, transition)
            .map_err(|e| CliError::internal(command, format!("apply: {e}")))?
    };

    record.envelope.updated_at = applied.occurred_at;

    // Persist with a TrustTransition history entry. `write_with_history`
    // re-hashes and atomically writes the file.
    let summary = format!(
        "{:?}→{:?}{}",
        applied.from,
        applied.to,
        applied
            .reason
            .as_deref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default(),
    );
    let draft = HistoryDraft {
        merged_via_pr: None,
        timestamp: applied.occurred_at,
        primary_actor: actor.clone(),
        contributors: Vec::new(),
        ops_summary: vec![summary],
        ops_count: 1,
        kind: HistoryEntryKind::TrustTransition,
    };
    let path = write_with_history(&ctx.storage, record, draft)
        .map_err(|e| CliError::internal(command, format!("write: {e}")))?;
    ctx.index
        .refresh(&ctx.storage, std::slice::from_ref(&path), &[])
        .map_err(|e| CliError::internal(command, format!("refresh: {e}")))?;
    Ok(())
}

/// Build an `Evidence` vec from an optional URL, attributing it to `actor`.
/// Returns an empty Vec when no URL was supplied (the trust machine will
/// reject high-stakes promotions in that case — by design).
fn build_evidence_url(
    actor: &Identity,
    url: Option<&str>,
    kind: Option<ft_core::EvidenceKind>,
) -> Vec<Evidence> {
    let Some(url) = url else { return Vec::new() };
    vec![Evidence {
        id: "ev-cli".to_string(),
        kind: kind.unwrap_or(ft_core::EvidenceKind::ManualNote),
        url: url.to_string(),
        description: None,
        created_at: Utc::now(),
        created_by: actor.clone(),
        commit_sha: None,
        symbol_name: None,
        content_hash: None,
    }]
}

/// Append-a-runbook-step helper used by `runbook step add`. Treated as an
/// `Update` history entry (not a trust transition).
pub fn runbook_step_add(
    args: &crate::cli::RunbookStepAddArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    const CMD: &str = "runbook step add";
    let mut ctx = WorkCtx::open(CMD, global.workspace.as_deref())?;
    let actor = ctx.actor()?;
    let id = ctx.resolve_id(&args.id)?;
    let mut record = ctx.read_record(&id)?;

    let RecordBody::Runbook(rb) = &mut record.body else {
        return Err(CliError::user(CMD, "record is not a runbook"));
    };
    rb.steps.push(ft_core::RunbookStep {
        description: args.description.clone(),
        command: args.command.clone(),
        expected_outcome: args.expected.clone(),
    });
    record.envelope.updated_at = Utc::now();

    let draft = HistoryDraft {
        merged_via_pr: None,
        timestamp: record.envelope.updated_at,
        primary_actor: actor,
        contributors: Vec::new(),
        ops_summary: vec![format!("added step: {}", args.description)],
        ops_count: 1,
        kind: HistoryEntryKind::Update,
    };
    let path = write_with_history(&ctx.storage, &mut record, draft)
        .map_err(|e| CliError::internal(CMD, format!("write: {e}")))?;
    ctx.index
        .refresh(&ctx.storage, std::slice::from_ref(&path), &[])
        .map_err(|e| CliError::internal(CMD, format!("refresh: {e}")))?;

    Ok(CommandOutcome::Updated(
        RecordOutcome::new(CMD, record).with_warnings(ctx.warnings.clone()),
    ))
}
