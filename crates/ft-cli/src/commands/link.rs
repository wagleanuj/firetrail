//! `firetrail link` and `firetrail dep {add,remove}`.
//!
//! Relations are persisted to the interim relation log (see [`crate::context`]
//! for details). The canonical relation store is `firetrail-tq7`.

use chrono::Utc;
use ft_core::{RecordId, Relation, RelationKind};
use serde::Serialize;

use crate::cli::{DepAddArgs, DepRemoveArgs, GlobalOpts, LinkArgs, RelationKindArg};
use crate::commands::CommandOutcome;
use crate::context::{WorkCtx, append_relation, load_relations, rewrite_relations};
use crate::error::CliError;

const COMMAND_LINK: &str = "link";
const COMMAND_DEP_ADD: &str = "dep add";
const COMMAND_DEP_REMOVE: &str = "dep remove";

/// `firetrail link`
pub fn link(args: &LinkArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    write_relation(COMMAND_LINK, global, &args.from, &args.to, args.kind, false)
}

/// `firetrail dep add`
pub fn dep_add(args: &DepAddArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    write_relation(
        COMMAND_DEP_ADD,
        global,
        &args.from,
        &args.to,
        args.kind,
        true,
    )
}

/// `firetrail dep remove`
pub fn dep_remove(args: &DepRemoveArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_DEP_REMOVE, global.workspace.as_deref())?;
    let from = ctx.resolve_id(&args.from)?;
    let to = ctx.resolve_id(&args.to)?;
    let kind_filter = args.kind.map(RelationKindArg::to_core);

    let existing = load_relations(&ctx.ws)?;
    let before = existing.len();
    let kept: Vec<Relation> = existing
        .into_iter()
        .filter(|r| {
            let same_endpoints = r.from == from && r.to == to;
            let kind_match = kind_filter.is_none_or(|k| r.kind == k);
            !(same_endpoints && kind_match)
        })
        .collect();
    let removed = before - kept.len();
    if removed == 0 {
        return Err(CliError::NotFound {
            command: COMMAND_DEP_REMOVE.into(),
            what: format!("relation {from} -> {to}"),
        });
    }
    rewrite_relations(&ctx.ws, &kept)?;
    // Removing a relation means the index has stale edges that an additive
    // `refresh()` can't undo (it only re-derives edges for *changed records*
    // and otherwise INSERT-OR-IGNOREs from the JSONL). Rebuild from storage
    // so the relations table reflects the rewritten log (firetrail-lr3).
    ctx.index
        .rebuild_from(&ctx.storage)
        .map_err(|e| CliError::internal(COMMAND_DEP_REMOVE, format!("rebuild: {e}")))?;
    Ok(CommandOutcome::RelationRemoved(RelationOutcome {
        command: COMMAND_DEP_REMOVE,
        from,
        to,
        kind: kind_filter.unwrap_or(RelationKind::RelatedTo),
        removed: u32::try_from(removed).unwrap_or(u32::MAX),
        warnings: ctx.warnings.clone(),
    }))
}

fn write_relation(
    command: &'static str,
    global: &GlobalOpts,
    from_raw: &str,
    to_raw: &str,
    kind: RelationKindArg,
    is_dep: bool,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(command, global.workspace.as_deref())?;
    let from = ctx.resolve_id(from_raw)?;
    let to = ctx.resolve_id(to_raw)?;
    if from == to {
        return Err(CliError::user(command, "self-edges are not allowed"));
    }
    let actor = ctx.actor()?;
    let core_kind = kind.to_core();
    if is_dep
        && !matches!(
            core_kind,
            RelationKind::Blocks
                | RelationKind::BlockedBy
                | RelationKind::ParentOf
                | RelationKind::ChildOf
        )
    {
        return Err(CliError::user(
            command,
            "dep relations must be one of: blocks, blocked-by, parent-of, child-of",
        ));
    }
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
    append_relation(&ctx.ws, &relation)?;
    // `refresh()` re-ingests the relations.jsonl log on every call, so an
    // otherwise-empty refresh surfaces the just-appended edge to subsequent
    // `ready` / `graph` / `walk` queries in the same session. Without this
    // call the index would only see the new edge after a manual
    // `firetrail index rebuild` (firetrail-lr3).
    ctx.index
        .refresh(&ctx.storage, &[], &[])
        .map_err(|e| CliError::internal(command, format!("refresh: {e}")))?;

    Ok(CommandOutcome::RelationAdded(RelationOutcome {
        command,
        from,
        to,
        kind: core_kind,
        removed: 0,
        warnings: ctx.warnings.clone(),
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationOutcome {
    pub command: &'static str,
    pub from: RecordId,
    pub to: RecordId,
    pub kind: RelationKind,
    /// Only set for remove operations.
    pub removed: u32,
    /// Non-fatal warnings (e.g. index auto-rebuild on open).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl RelationOutcome {
    pub fn markdown(&self) -> String {
        if self.command == COMMAND_DEP_REMOVE {
            format!(
                "removed {} relation(s) from `{}` -> `{}`\n",
                self.removed, self.from, self.to
            )
        } else {
            let kind = serde_json::to_value(self.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:?}", self.kind));
            format!("`{}` --{}--> `{}`\n", self.from, kind, self.to)
        }
    }

    pub fn quiet_line(&self) -> String {
        if self.command == COMMAND_DEP_REMOVE {
            format!("removed {} relation(s)", self.removed)
        } else {
            format!("{} -> {}", self.from, self.to)
        }
    }
}
