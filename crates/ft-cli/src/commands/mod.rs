//! Command handlers.
//!
//! Every subcommand returns a [`CommandOutcome`]. The outcome carries both a
//! markdown rendering and the JSON shape so that the [`crate::output`]
//! formatter can pick the right representation without command handlers
//! needing to know which format is selected.

pub mod board;
pub mod claim;
pub mod close;
pub mod create;
pub mod criteria;
pub mod doctor;
pub mod graph;
pub mod init;
pub mod link;
pub mod list;
pub mod show;
pub mod update;

use ft_core::Record;
use serde::Serialize;
use serde_json::Value;

use crate::cli::PriorityArg;

/// Helper: bridge clap `PriorityArg` -> core `Priority`. Kept here so per-
/// command modules can call it without re-importing the enum directly.
pub fn priority_to_core(p: PriorityArg) -> ft_core::Priority {
    p.to_core()
}

/// Wrapper for `RecordOutcome` JSON serialization that exposes the canonical id.
#[derive(Debug, Clone, Serialize)]
pub struct RecordOutcome {
    /// Stable command name (e.g. `"task create"`).
    #[serde(skip)]
    pub command: &'static str,
    /// The record itself (already includes its id and state hash).
    pub record: Record,
    /// Non-fatal warnings to surface in the JSON envelope (e.g. auto-rebuild
    /// of `index.db`). Defaults empty.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl RecordOutcome {
    /// Build a [`RecordOutcome`] with no warnings.
    pub fn new(command: &'static str, record: Record) -> Self {
        Self {
            command,
            record,
            warnings: Vec::new(),
        }
    }

    /// Attach warnings (e.g. from [`crate::context::WorkCtx::warnings`]) to
    /// this outcome.
    #[must_use]
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        format!(
            "**{}** `{}` ({:?}) — state_hash `{}`\n",
            self.command,
            self.record.envelope.id,
            self.record.envelope.status,
            self.record.envelope.state_hash
        )
    }
    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!("{} {}", self.command, self.record.envelope.id)
    }
}

/// Outcome of a successful subcommand.
#[derive(Debug, Clone)]
pub enum CommandOutcome {
    /// `firetrail init`.
    Init(init::InitReport),
    /// `firetrail doctor`.
    Doctor(doctor::DoctorReport),
    /// A record was just created.
    Created(RecordOutcome),
    /// A record was updated in place.
    Updated(RecordOutcome),
    /// A record was closed.
    Closed(RecordOutcome),
    /// A record was claimed.
    Claimed(RecordOutcome),
    /// Acceptance-criteria listing.
    CriteriaList(criteria::CriteriaListOutcome),
    /// A relation was added.
    RelationAdded(link::RelationOutcome),
    /// A relation was removed.
    RelationRemoved(link::RelationOutcome),
    /// `firetrail show`.
    Show(show::ShowOutcome),
    /// `firetrail list` / `firetrail ready`.
    List(list::ListOutcome),
    /// `firetrail board`.
    Board(board::BoardOutcome),
    /// `firetrail graph`.
    Graph(graph::GraphOutcome),
}

impl CommandOutcome {
    /// Stable command name for the JSON envelope's `command` field.
    pub fn command(&self) -> &'static str {
        match self {
            Self::Init(_) => "init",
            Self::Doctor(_) => "doctor",
            Self::Created(r) | Self::Updated(r) | Self::Closed(r) | Self::Claimed(r) => r.command,
            Self::CriteriaList(_) => "criteria list",
            Self::RelationAdded(r) | Self::RelationRemoved(r) => r.command,
            Self::Show(_) => "show",
            Self::List(l) => l.command,
            Self::Board(_) => "board",
            Self::Graph(_) => "graph",
        }
    }

    /// Markdown rendering for human consumption.
    pub fn markdown(&self) -> String {
        match self {
            Self::Init(r) => r.markdown(),
            Self::Doctor(r) => r.markdown(),
            Self::Created(r) | Self::Updated(r) | Self::Closed(r) | Self::Claimed(r) => {
                r.markdown()
            }
            Self::CriteriaList(c) => c.markdown(),
            Self::RelationAdded(r) | Self::RelationRemoved(r) => r.markdown(),
            Self::Show(s) => s.markdown(),
            Self::List(l) => l.markdown(),
            Self::Board(b) => b.markdown(),
            Self::Graph(g) => g.markdown(),
        }
    }

    /// One-line markdown used in `--quiet` mode.
    pub fn quiet_line(&self) -> String {
        match self {
            Self::Init(r) => r.quiet_line(),
            Self::Doctor(r) => r.quiet_line(),
            Self::Created(r) | Self::Updated(r) | Self::Closed(r) | Self::Claimed(r) => {
                r.quiet_line()
            }
            Self::CriteriaList(c) => c.quiet_line(),
            Self::RelationAdded(r) | Self::RelationRemoved(r) => r.quiet_line(),
            Self::Show(s) => s.quiet_line(),
            Self::List(l) => l.quiet_line(),
            Self::Board(b) => b.quiet_line(),
            Self::Graph(g) => g.quiet_line(),
        }
    }

    /// JSON payload (placed under `data` in the envelope).
    pub fn json_data(&self) -> Value {
        match self {
            Self::Init(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::Doctor(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::Created(r) | Self::Updated(r) | Self::Closed(r) | Self::Claimed(r) => {
                serde_json::to_value(r).unwrap_or(Value::Null)
            }
            Self::CriteriaList(c) => serde_json::to_value(c).unwrap_or(Value::Null),
            Self::RelationAdded(r) | Self::RelationRemoved(r) => {
                serde_json::to_value(r).unwrap_or(Value::Null)
            }
            Self::Show(s) => serde_json::to_value(s).unwrap_or(Value::Null),
            Self::List(l) => serde_json::to_value(l).unwrap_or(Value::Null),
            Self::Board(b) => serde_json::to_value(b).unwrap_or(Value::Null),
            Self::Graph(g) => serde_json::to_value(g).unwrap_or(Value::Null),
        }
    }

    /// Non-fatal warnings for the JSON envelope.
    pub fn warnings(&self) -> Vec<String> {
        match self {
            Self::Init(r) => r.warnings.clone(),
            Self::Doctor(r) => r.warnings.clone(),
            Self::Created(r) | Self::Updated(r) | Self::Closed(r) | Self::Claimed(r) => {
                r.warnings.clone()
            }
            Self::RelationAdded(r) | Self::RelationRemoved(r) => r.warnings.clone(),
            Self::CriteriaList(c) => c.warnings.clone(),
            Self::Show(s) => s.warnings.clone(),
            Self::List(l) => l.warnings.clone(),
            Self::Board(b) => b.warnings.clone(),
            Self::Graph(g) => g.warnings.clone(),
        }
    }
}
