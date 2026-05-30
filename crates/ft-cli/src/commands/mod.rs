//! Command handlers.
//!
//! Every subcommand returns a [`CommandOutcome`]. The outcome carries both a
//! markdown rendering and the JSON shape so that the [`crate::output`]
//! formatter can pick the right representation without command handlers
//! needing to know which format is selected.

pub mod board;
pub mod check;
pub mod claim;
pub mod close;
pub mod compact;
pub mod create;
pub mod criteria;
pub mod daemon_cmd;
pub mod diff;
pub mod doc;
pub mod doctor;
pub mod graph;
pub mod hook;
pub mod identity;
pub mod import_cmd;
pub mod index_cmd;
pub mod init;
pub mod link;
pub mod lint;
pub mod list;
pub mod memory_create;
pub mod memory_views;
pub mod merge_driver;
pub mod migrate;
pub mod prime;
pub mod promote_import;
pub mod review;
pub mod salvage;
pub mod scope;
pub mod search;
pub mod server_hooks;
pub mod show;
pub mod sync_cmd;
pub mod trust;
pub mod ui;
pub mod update;
pub mod verify;

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
    /// `firetrail verify`.
    Verify(verify::VerifyOutcome),
    /// `firetrail compact`.
    Compact(compact::CompactOutcome),
    /// `firetrail check pr`.
    CheckPr(check::CheckPrOutcome),
    /// `firetrail check paths`.
    CheckPaths(check::CheckPathsOutcome),
    /// `firetrail diff`.
    Diff(diff::DiffOutcome),
    /// `firetrail lint memory`.
    LintMemory(lint::LintMemoryOutcome),
    /// `firetrail review`.
    Review(review::ReviewOutcome),
    /// `firetrail merge-driver-install`.
    MergeDriverInstall(merge_driver::MergeDriverInstallOutcome),
    /// `firetrail server-hooks install`.
    ServerHooks(server_hooks::ServerHooksOutcome),
    /// `firetrail memory list` / `memory stale`.
    MemoryList(memory_views::MemoryListOutcome),
    /// `firetrail memory show`.
    MemoryShow(memory_views::MemoryShowOutcome),
    /// `firetrail memory salvage`.
    MemorySalvage(salvage::SalvageOutcome),
    /// `firetrail _hook on-checkout` / `_hook on-merge`.
    Hook(hook::HookOutcome),
    /// `firetrail search` / `firetrail similar`.
    Search(search::SearchOutcome),
    /// `firetrail prime`.
    Prime(prime::PrimeOutcome),
    /// `firetrail index rebuild` / `firetrail index refresh`.
    IndexAction(index_cmd::IndexActionOutcome),
    /// `firetrail doc add` / `link` / `index`.
    Doc(doc::DocOutcome),
    /// `firetrail daemon {start,stop,status}`.
    Daemon(daemon_cmd::DaemonOutcome),
    /// `firetrail ui`.
    Ui(ui::UiOutcome),
    /// `firetrail identity register`.
    IdentityRegister(identity::IdentityRegisterOutcome),
    /// `firetrail identity list`.
    IdentityList(identity::IdentityListOutcome),
    /// `firetrail identity show`.
    IdentityShow(identity::IdentityShowOutcome),
    /// `firetrail identity offboard`.
    IdentityOffboard(identity::IdentityOffboardOutcome),
    /// `firetrail scope list`.
    ScopeList(scope::ScopeListOutcome),
    /// `firetrail scope show`.
    ScopeShow(scope::ScopeShowOutcome),
    /// `firetrail scope aliases`.
    ScopeAliases(scope::ScopeAliasesOutcome),
    /// `firetrail scope owners`.
    ScopeOwners(scope::ScopeOwnersOutcome),
    /// `firetrail sync`.
    Sync(sync_cmd::SyncOutcome),
    /// `firetrail import …` (M6).
    Import(import_cmd::ImportOutcome),
    /// `firetrail promote-import` (M6).
    PromoteImport(promote_import::PromoteImportOutcome),
    /// `firetrail migrate embeddings` (firetrail-vpn).
    Migrate(migrate::MigrateEmbeddingsOutcome),
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
            Self::Verify(_) => "verify",
            Self::Compact(_) => "compact",
            Self::CheckPr(_) => "check pr",
            Self::CheckPaths(_) => "check paths",
            Self::Diff(_) => "diff",
            Self::LintMemory(_) => "lint memory",
            Self::Review(_) => "review",
            Self::MergeDriverInstall(_) => "merge-driver-install",
            Self::ServerHooks(_) => "server-hooks install",
            Self::MemoryList(l) => l.command,
            Self::MemoryShow(_) => "memory show",
            Self::MemorySalvage(_) => "memory salvage",
            Self::Hook(h) => h.command,
            Self::Search(s) => s.command,
            Self::Prime(_) => "prime",
            Self::IndexAction(i) => i.command,
            Self::Doc(d) => d.command,
            Self::Daemon(d) => d.command,
            Self::Ui(_) => "ui",
            Self::IdentityRegister(r) => r.command,
            Self::IdentityList(r) => r.command,
            Self::IdentityShow(r) => r.command,
            Self::IdentityOffboard(r) => r.command,
            Self::ScopeList(r) => r.command,
            Self::ScopeShow(r) => r.command,
            Self::ScopeAliases(r) => r.command,
            Self::ScopeOwners(r) => r.command,
            Self::Sync(r) => r.command,
            Self::Import(r) => r.command,
            Self::PromoteImport(r) => r.command,
            Self::Migrate(r) => r.command,
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
            Self::Verify(v) => v.markdown(),
            Self::Compact(c) => c.markdown(),
            Self::CheckPr(c) => c.markdown(),
            Self::CheckPaths(c) => c.markdown(),
            Self::Diff(d) => d.markdown(),
            Self::LintMemory(l) => l.markdown(),
            Self::Review(r) => r.markdown(),
            Self::MergeDriverInstall(m) => m.markdown(),
            Self::ServerHooks(s) => s.markdown(),
            Self::MemoryList(l) => l.markdown(),
            Self::MemoryShow(s) => s.markdown(),
            Self::MemorySalvage(s) => s.markdown(),
            Self::Hook(h) => h.markdown(),
            Self::Search(s) => s.markdown(),
            Self::Prime(p) => p.markdown(),
            Self::IndexAction(i) => i.markdown(),
            Self::Doc(d) => d.markdown(),
            Self::Daemon(d) => d.markdown(),
            Self::Ui(u) => u.markdown(),
            Self::IdentityRegister(r) => r.markdown(),
            Self::IdentityList(r) => r.markdown(),
            Self::IdentityShow(r) => r.markdown(),
            Self::IdentityOffboard(r) => r.markdown(),
            Self::ScopeList(r) => r.markdown(),
            Self::ScopeShow(r) => r.markdown(),
            Self::ScopeAliases(r) => r.markdown(),
            Self::ScopeOwners(r) => r.markdown(),
            Self::Sync(r) => r.markdown(),
            Self::Import(r) => r.markdown(),
            Self::PromoteImport(r) => r.markdown(),
            Self::Migrate(r) => r.markdown(),
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
            Self::Verify(v) => v.quiet_line(),
            Self::Compact(c) => c.quiet_line(),
            Self::CheckPr(c) => c.quiet_line(),
            Self::CheckPaths(c) => c.quiet_line(),
            Self::Diff(d) => d.quiet_line(),
            Self::LintMemory(l) => l.quiet_line(),
            Self::Review(r) => r.quiet_line(),
            Self::MergeDriverInstall(m) => m.quiet_line(),
            Self::ServerHooks(s) => s.quiet_line(),
            Self::MemoryList(l) => l.quiet_line(),
            Self::MemoryShow(s) => s.quiet_line(),
            Self::MemorySalvage(s) => s.quiet_line(),
            Self::Hook(h) => h.quiet_line(),
            Self::Search(s) => s.quiet_line(),
            Self::Prime(p) => p.quiet_line(),
            Self::IndexAction(i) => i.quiet_line(),
            Self::Doc(d) => d.quiet_line(),
            Self::Daemon(d) => d.quiet_line(),
            Self::Ui(u) => u.quiet_line(),
            Self::IdentityRegister(r) => r.quiet_line(),
            Self::IdentityList(r) => r.quiet_line(),
            Self::IdentityShow(r) => r.quiet_line(),
            Self::IdentityOffboard(r) => r.quiet_line(),
            Self::ScopeList(r) => r.quiet_line(),
            Self::ScopeShow(r) => r.quiet_line(),
            Self::ScopeAliases(r) => r.quiet_line(),
            Self::ScopeOwners(r) => r.quiet_line(),
            Self::Sync(r) => r.quiet_line(),
            Self::Import(r) => r.quiet_line(),
            Self::PromoteImport(r) => r.quiet_line(),
            Self::Migrate(r) => r.quiet_line(),
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
            Self::Verify(v) => serde_json::to_value(v).unwrap_or(Value::Null),
            Self::Compact(c) => serde_json::to_value(c).unwrap_or(Value::Null),
            Self::CheckPr(c) => serde_json::to_value(c).unwrap_or(Value::Null),
            Self::CheckPaths(c) => serde_json::to_value(c).unwrap_or(Value::Null),
            Self::Diff(d) => serde_json::to_value(d).unwrap_or(Value::Null),
            Self::LintMemory(l) => serde_json::to_value(l).unwrap_or(Value::Null),
            Self::Review(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::MergeDriverInstall(m) => serde_json::to_value(m).unwrap_or(Value::Null),
            Self::ServerHooks(s) => serde_json::to_value(s).unwrap_or(Value::Null),
            Self::MemoryList(l) => serde_json::to_value(l).unwrap_or(Value::Null),
            Self::MemoryShow(s) => serde_json::to_value(s).unwrap_or(Value::Null),
            Self::MemorySalvage(s) => serde_json::to_value(s).unwrap_or(Value::Null),
            Self::Hook(h) => serde_json::to_value(h).unwrap_or(Value::Null),
            Self::Search(s) => serde_json::to_value(s).unwrap_or(Value::Null),
            Self::Prime(p) => p.json_data(),
            Self::IndexAction(i) => serde_json::to_value(i).unwrap_or(Value::Null),
            Self::Doc(d) => serde_json::to_value(d).unwrap_or(Value::Null),
            Self::Daemon(d) => serde_json::to_value(d).unwrap_or(Value::Null),
            Self::Ui(u) => serde_json::to_value(u).unwrap_or(Value::Null),
            Self::IdentityRegister(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::IdentityList(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::IdentityShow(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::IdentityOffboard(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::ScopeList(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::ScopeShow(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::ScopeAliases(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::ScopeOwners(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::Sync(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::Import(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::PromoteImport(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Self::Migrate(r) => serde_json::to_value(r).unwrap_or(Value::Null),
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
            Self::Verify(v) => v.warnings.clone(),
            Self::Compact(c) => c.warnings.clone(),
            Self::CheckPr(c) => c.warnings.clone(),
            Self::CheckPaths(c) => c.warnings.clone(),
            Self::Diff(d) => d.warnings.clone(),
            Self::LintMemory(l) => l.warnings.clone(),
            Self::Review(r) => r.warnings.clone(),
            Self::MergeDriverInstall(m) => m.warnings.clone(),
            Self::ServerHooks(s) => s.warnings.clone(),
            Self::MemoryList(l) => l.warnings.clone(),
            Self::MemoryShow(s) => s.warnings.clone(),
            Self::MemorySalvage(s) => s.warnings.clone(),
            Self::Hook(h) => h.warnings.clone(),
            Self::Search(s) => s.warnings.clone(),
            Self::Prime(p) => p.warnings.clone(),
            Self::IndexAction(i) => i.warnings.clone(),
            Self::Doc(d) => d.warnings.clone(),
            Self::Daemon(d) => d.warnings.clone(),
            Self::Ui(u) => u.warnings.clone(),
            Self::IdentityRegister(r) => r.warnings.clone(),
            Self::IdentityList(r) => r.warnings.clone(),
            Self::IdentityShow(r) => r.warnings.clone(),
            Self::IdentityOffboard(r) => r.warnings.clone(),
            Self::ScopeList(r) => r.warnings.clone(),
            Self::ScopeShow(r) => r.warnings.clone(),
            Self::ScopeAliases(r) => r.warnings.clone(),
            Self::ScopeOwners(r) => r.warnings.clone(),
            Self::Sync(r) => r.warnings.clone(),
            Self::Import(r) => r.warnings.clone(),
            Self::PromoteImport(r) => r.warnings.clone(),
            Self::Migrate(r) => r.warnings.clone(),
        }
    }
}
