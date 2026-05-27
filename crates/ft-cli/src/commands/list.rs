//! `firetrail list` and `firetrail ready` — index-backed query commands.

use ft_core::Identity;
use ft_index::{IndexedRecord, ListQuery, ReadyQuery};
use serde::Serialize;

use crate::cli::{GlobalOpts, ListArgs, ReadyArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND_LIST: &str = "list";
const COMMAND_READY: &str = "ready";

/// `firetrail list`
pub fn list(args: &ListArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND_LIST, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let mut q = ListQuery::default();
    if let Some(k) = args.kind {
        q.kinds = Some(vec![k.to_core()]);
    }
    if let Some(s) = args.status {
        q.statuses = Some(vec![s.to_core()]);
        // status filter implies the user knows what they want.
        q.include_closed = true;
        q.include_archived = true;
    }
    if let Some(o) = &args.owner {
        let identity = Identity::new(o.clone())
            .map_err(|e| CliError::user(COMMAND_LIST, format!("invalid owner: {e}")))?;
        q.owners = Some(vec![identity]);
    }
    if let Some(s) = &args.scope {
        q.scopes = Some(vec![s.clone()]);
    }
    q.limit = args.limit;
    q.offset = args.offset;

    let rows = ctx
        .index
        .list(&q)
        .map_err(|e| CliError::internal(COMMAND_LIST, e))?;
    Ok(CommandOutcome::List(ListOutcome {
        command: COMMAND_LIST,
        rows: rows.into_iter().map(IndexedRow::from).collect(),
        warnings,
    }))
}

/// `firetrail ready`
pub fn ready(args: &ReadyArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND_READY, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let mut q = ReadyQuery::default();
    if let Some(k) = args.kind {
        q.kinds = Some(vec![k.to_core()]);
    }
    if let Some(o) = &args.owner {
        let identity = Identity::new(o.clone())
            .map_err(|e| CliError::user(COMMAND_READY, format!("invalid owner: {e}")))?;
        q.owners = Some(vec![identity]);
    }
    if let Some(s) = &args.scope {
        q.scopes = Some(vec![s.clone()]);
    }
    q.limit = args.limit;

    let rows = ctx
        .index
        .ready(&q)
        .map_err(|e| CliError::internal(COMMAND_READY, e))?;
    Ok(CommandOutcome::List(ListOutcome {
        command: COMMAND_READY,
        rows: rows.into_iter().map(IndexedRow::from).collect(),
        warnings,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexedRow {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub owner: Option<String>,
    pub scope: Option<String>,
}

impl From<IndexedRecord> for IndexedRow {
    fn from(r: IndexedRecord) -> Self {
        Self {
            id: r.id.as_str().to_string(),
            kind: serde_value_str(&r.kind),
            title: r.title,
            status: serde_value_str(&r.status),
            priority: serde_value_str(&r.priority),
            owner: r.owner.map(|o| o.as_str().to_string()),
            scope: r.owning_scope,
        }
    }
}

/// Render an enum via its serde representation (`snake_case` for
/// kind/status, lowercase for priority).
fn serde_value_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
pub struct ListOutcome {
    pub command: &'static str,
    pub rows: Vec<IndexedRow>,
    /// Non-fatal warnings (e.g. index auto-rebuild on open).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ListOutcome {
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        if self.rows.is_empty() {
            return "_no records match_\n".to_string();
        }
        let mut s = String::from("| ID | Kind | Status | Priority | Owner | Title |\n");
        s.push_str("|----|------|--------|----------|-------|-------|\n");
        for r in &self.rows {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} | {} | {} |",
                r.id,
                r.kind,
                r.status,
                r.priority,
                r.owner.as_deref().unwrap_or("—"),
                r.title.replace('|', "\\|"),
            );
        }
        s
    }

    pub fn quiet_line(&self) -> String {
        format!("{}: {} record(s)", self.command, self.rows.len())
    }
}
