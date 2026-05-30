//! `firetrail memory {list,stale,show}` — read-only views.
//!
//! Walks storage directly (rather than the index) because trust / risk
//! filtering needs to inspect the body, and the index does not surface
//! those fields at M2.

use chrono::Utc;
use ft_core::{Record, RecordBody, RecordKind, RiskClass, TrustState};
use ft_storage::{Storage as _, StorageFilter};
use ft_trust::{StalePolicy, is_stale};
use serde::Serialize;

use crate::cli::{GlobalOpts, MemoryListArgs, MemoryShowArgs, MemoryStaleArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_LIST: &str = "memory list";
const CMD_STALE: &str = "memory stale";
const CMD_SHOW: &str = "memory show";

/// All memory kinds. Drives the default scan when `--kind` is not set.
const MEMORY_KINDS: &[RecordKind] = &[
    RecordKind::Incident,
    RecordKind::Finding,
    RecordKind::Runbook,
    RecordKind::Decision,
    RecordKind::Gotcha,
    RecordKind::Memory,
];

/// `firetrail memory list`
pub fn list(args: &MemoryListArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_LIST, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let mut filter = StorageFilter::default();
    if let Some(k) = args.kind {
        filter = filter.kind(k.to_core());
    } else {
        for k in MEMORY_KINDS {
            filter = filter.kind(*k);
        }
    }

    let ids = ctx
        .storage
        .list(&filter)
        .map_err(|e| CliError::internal(CMD_LIST, format!("list: {e}")))?;

    let policy = StalePolicy::default();
    let now = Utc::now();
    let mut rows: Vec<MemoryRow> = Vec::new();
    for id in ids {
        let Ok(record) = ctx.storage.read(&id) else {
            continue;
        };
        let trust = body_trust(&record.body);
        let risk = body_risk(&record.body);
        let stale = is_stale(&record, now, &policy);

        if let Some(want) = args.trust {
            if trust.as_ref() != Some(&want.to_core()) {
                continue;
            }
        }
        if let Some(want) = args.risk_class {
            if risk.as_ref() != Some(&want.to_core()) {
                continue;
            }
        }
        if args.stale && !stale {
            continue;
        }
        rows.push(MemoryRow::from_record(&record, trust, risk, stale));
        if let Some(limit) = args.limit {
            if rows.len() as u64 >= limit {
                break;
            }
        }
    }

    Ok(CommandOutcome::MemoryList(MemoryListOutcome {
        command: CMD_LIST,
        rows,
        warnings,
    }))
}

/// `firetrail memory stale`
pub fn stale(args: &MemoryStaleArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_STALE, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let mut filter = StorageFilter::default();
    if let Some(k) = args.kind {
        filter = filter.kind(k.to_core());
    } else {
        for k in MEMORY_KINDS {
            filter = filter.kind(*k);
        }
    }

    let ids = ctx
        .storage
        .list(&filter)
        .map_err(|e| CliError::internal(CMD_STALE, format!("list: {e}")))?;
    let policy = StalePolicy::default();
    let now = Utc::now();

    let mut rows: Vec<MemoryRow> = Vec::new();
    for id in ids {
        let Ok(record) = ctx.storage.read(&id) else {
            continue;
        };
        if !is_stale(&record, now, &policy) {
            continue;
        }
        let trust = body_trust(&record.body);
        let risk = body_risk(&record.body);
        rows.push(MemoryRow::from_record(&record, trust, risk, true));
    }

    Ok(CommandOutcome::MemoryList(MemoryListOutcome {
        command: CMD_STALE,
        rows,
        warnings,
    }))
}

/// `firetrail memory show`
pub fn show(args: &MemoryShowArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_SHOW, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let id = ctx.resolve_id(&args.id)?;
    let record = ctx.read_record(&id)?;
    Ok(CommandOutcome::MemoryShow(MemoryShowOutcome {
        record,
        warnings,
    }))
}

/// Per-record row in `memory list` / `memory stale`.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryRow {
    /// Canonical record id.
    pub id: String,
    /// Memory kind (`incident`, `finding`, …).
    pub kind: String,
    /// Short title / summary.
    pub title: String,
    /// Trust state.
    pub trust: Option<String>,
    /// Risk class.
    pub risk_class: Option<String>,
    /// `true` when [`is_stale`] returned `true` for this record.
    pub stale: bool,
}

impl MemoryRow {
    fn from_record(
        record: &Record,
        trust: Option<TrustState>,
        risk: Option<RiskClass>,
        stale: bool,
    ) -> Self {
        Self {
            id: record.envelope.id.as_str().to_string(),
            kind: serialize_str(&record.envelope.kind),
            title: record.envelope.title.clone(),
            trust: trust.map(|t| serialize_str(&t)),
            risk_class: risk.map(|r| serialize_str(&r)),
            stale,
        }
    }
}

fn serialize_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Outcome of `memory list` / `memory stale`.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryListOutcome {
    /// Stable command name for the JSON envelope.
    #[serde(skip)]
    pub command: &'static str,
    /// Matching rows.
    pub rows: Vec<MemoryRow>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl MemoryListOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        if self.rows.is_empty() {
            return "_no records match_\n".to_string();
        }
        let mut s = String::from(
            "| ID | Kind | Trust | Risk | Stale | Title |\n|----|------|-------|------|-------|-------|\n",
        );
        for r in &self.rows {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} | {} | {} |",
                r.id,
                r.kind,
                r.trust.as_deref().unwrap_or("—"),
                r.risk_class.as_deref().unwrap_or("—"),
                if r.stale { "✓" } else { "—" },
                r.title.replace('|', "\\|"),
            );
        }
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!("{}: {} record(s)", self.command, self.rows.len())
    }
}

/// Outcome of `memory show` — the full record plus a kind-specific
/// markdown body block.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryShowOutcome {
    /// The record itself.
    pub record: Record,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl MemoryShowOutcome {
    /// Markdown rendering, including a kind-appropriate body block.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let env = &self.record.envelope;
        let mut s = format!(
            "# {kind:?} `{id}`\n\n**{title}**\n\nstatus: {status:?} · priority: {priority:?} · origin: {origin:?}\n",
            kind = env.kind,
            id = env.id,
            title = env.title,
            status = env.status,
            priority = env.priority,
            origin = env.origin,
        );
        let _ = writeln!(s, "updated_at: {}", env.updated_at.to_rfc3339());

        match &self.record.body {
            RecordBody::Incident(b) => {
                let _ = writeln!(
                    s,
                    "\n## Incident\n\nseverity: {:?}\nstarted_at: {}\ntrust: {:?}\n",
                    b.severity,
                    b.started_at.to_rfc3339(),
                    b.trust
                );
                let _ = writeln!(s, "\n{}", b.summary);
                if let Some(rc) = &b.root_cause {
                    let _ = writeln!(s, "\n### Root cause\n\n{rc}");
                }
            }
            RecordBody::Finding(b) => {
                let _ = writeln!(s, "\n## Finding\n\ntrust: {:?}\n", b.trust);
                let _ = writeln!(s, "\n{}\n", b.summary);
                if !b.details.is_empty() {
                    let _ = writeln!(s, "\n{}", b.details);
                }
            }
            RecordBody::Runbook(b) => {
                let _ = writeln!(s, "\n## Runbook\n\ntrust: {:?}\n", b.trust);
                let _ = writeln!(s, "\n{}\n", b.summary);
                for (i, step) in b.steps.iter().enumerate() {
                    let _ = writeln!(s, "\n### Step {}: {}\n", i + 1, step.description);
                    if let Some(cmd) = &step.command {
                        let _ = writeln!(s, "```\n{cmd}\n```");
                    }
                    let _ = writeln!(s, "_expected:_ {}", step.expected_outcome);
                }
            }
            RecordBody::Decision(b) => {
                let _ = writeln!(
                    s,
                    "\n## Decision\n\nstatus: {:?} · trust: {:?}\n",
                    b.status, b.trust
                );
                if !b.context.is_empty() {
                    let _ = writeln!(s, "\n### Context\n\n{}", b.context);
                }
                let _ = writeln!(s, "\n### Decision\n\n{}", b.decision);
                if !b.consequences.is_empty() {
                    let _ = writeln!(s, "\n### Consequences\n\n{}", b.consequences);
                }
            }
            RecordBody::Gotcha(b) => {
                let _ = writeln!(s, "\n## Gotcha\n\ntrust: {:?}\n\n{}", b.trust, b.summary);
                if !b.details.is_empty() {
                    let _ = writeln!(s, "\n{}", b.details);
                }
            }
            RecordBody::Memory(b) => {
                let _ = writeln!(s, "\n## Memory\n\ntrust: {:?}\n\n{}", b.trust, b.body);
            }
            // Non-memory bodies: render a minimal envelope summary.
            _ => {
                let _ = writeln!(
                    s,
                    "\n_(non-memory body — use `firetrail show` for full detail)_"
                );
            }
        }

        let _ = writeln!(s, "\n## State hash\n`{}`", env.state_hash);
        s
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "{}: {}",
            self.record.envelope.id, self.record.envelope.title
        )
    }
}

fn body_trust(body: &RecordBody) -> Option<TrustState> {
    match body {
        RecordBody::Incident(b) => Some(b.trust),
        RecordBody::Finding(b) => Some(b.trust),
        RecordBody::Runbook(b) => Some(b.trust),
        RecordBody::Decision(b) => Some(b.trust),
        RecordBody::Gotcha(b) => Some(b.trust),
        RecordBody::Memory(b) => Some(b.trust),
        _ => None,
    }
}

fn body_risk(body: &RecordBody) -> Option<RiskClass> {
    match body {
        RecordBody::Incident(b) => b.risk_class,
        RecordBody::Finding(b) => b.risk_class,
        RecordBody::Runbook(b) => b.risk_class,
        RecordBody::Decision(b) => b.risk_class,
        RecordBody::Gotcha(b) => b.risk_class,
        RecordBody::Memory(b) => b.risk_class,
        _ => None,
    }
}
