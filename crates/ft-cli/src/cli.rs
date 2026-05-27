//! Clap argument types for the `firetrail` binary.
//!
//! New subcommands plug in by adding a variant to [`Command`] and a handler
//! under `crate::commands`. The dispatcher in `main.rs` matches on [`Command`]
//! and delegates; nothing else needs to change to add a command.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Output format requested by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    /// Human-readable Markdown (default when stdout is a TTY).
    Markdown,
    /// Machine-readable JSON (default when stdout is not a TTY).
    Json,
}

/// Options shared across every subcommand.
#[derive(Debug, Args)]
pub struct GlobalOpts {
    /// Output format. Auto-detected when omitted (markdown on a TTY, json otherwise).
    #[arg(long, value_enum, global = true)]
    pub format: Option<FormatArg>,

    /// Shortcut for `--format json`.
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-essential output.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Enable verbose diagnostics (enables `tracing` to stderr).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Override the workspace root (default: discover from cwd).
    #[arg(long, global = true, value_name = "PATH")]
    pub workspace: Option<PathBuf>,
}

/// Root parser for the `firetrail` binary.
#[derive(Debug, Parser)]
#[command(
    name = "firetrail",
    version,
    about = "Firetrail — work-graph + memory CLI",
    long_about = None,
    arg_required_else_help = true,
)]
pub struct Cli {
    /// Global options that apply to every subcommand.
    #[command(flatten)]
    pub global: GlobalOpts,

    /// Selected subcommand.
    #[command(subcommand)]
    pub command: Command,
}

/// All `firetrail` subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialise a Firetrail workspace in the current git repo.
    Init(InitArgs),

    /// Verify the workspace is healthy and report any actionable issues.
    Doctor(DoctorArgs),

    /// Create / inspect epics.
    #[command(subcommand)]
    Epic(EpicCmd),

    /// Create / inspect tasks.
    #[command(subcommand)]
    Task(TaskCmd),

    /// Create / inspect subtasks.
    #[command(subcommand)]
    Subtask(SubtaskCmd),

    /// Create / inspect bugs.
    #[command(subcommand)]
    Bug(BugCmd),

    /// Update an existing record's fields.
    Update(UpdateArgs),

    /// Close a record (validates acceptance criteria).
    Close(CloseArgs),

    /// Re-open a closed record.
    Reopen(ReopenArgs),

    /// Claim a record (mints a Claim).
    Claim(ClaimArgs),

    /// Release the active claim on a record.
    Unclaim(UnclaimArgs),

    /// Acceptance-criteria management.
    #[command(subcommand)]
    Criteria(CriteriaCmd),

    /// Create a relation between two records.
    Link(LinkArgs),

    /// Dependency-relation shortcuts.
    #[command(subcommand)]
    Dep(DepCmd),

    /// Show a record's full envelope, body, and relations.
    Show(ShowArgs),

    /// List records matching a filter.
    List(ListArgs),

    /// List records ready to be picked up (no active blockers).
    Ready(ReadyArgs),

    /// Render a kanban-style board.
    Board(BoardArgs),

    /// Render an ASCII dependency tree.
    Graph(GraphArgs),

    /// Create an incident memory record.
    #[command(subcommand)]
    Incident(IncidentCmd),

    /// Create a finding memory record.
    #[command(subcommand)]
    Finding(FindingCmd),

    /// Create / manage a runbook memory record.
    #[command(subcommand)]
    Runbook(RunbookCmd),

    /// Create a decision memory record.
    #[command(subcommand)]
    Decision(DecisionCmd),

    /// Create a gotcha memory record.
    #[command(subcommand)]
    Gotcha(GotchaCmd),

    /// Memory-record management: create, list, show, lifecycle.
    #[command(subcommand)]
    Memory(MemoryCmd),

    /// Quick opportunistic memory capture.
    Capture(CaptureArgs),

    /// Verify per-record history chain integrity.
    Verify(VerifyArgs),

    /// PR-time history compaction.
    Compact(CompactArgs),

    /// Workspace / PR sanity checks.
    #[command(subcommand)]
    Check(CheckCmd),
}

/// Severity selector for `incident create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum SeverityArg {
    /// `sev1` — customer-impacting outage.
    Sev1,
    /// `sev2` — major degradation.
    Sev2,
    /// `sev3` — minor impact.
    Sev3,
    /// `sev4` — informational.
    Sev4,
}

impl SeverityArg {
    /// Convert to `ft_core::Severity`.
    #[must_use]
    pub fn to_core(self) -> ft_core::Severity {
        match self {
            Self::Sev1 => ft_core::Severity::Sev1,
            Self::Sev2 => ft_core::Severity::Sev2,
            Self::Sev3 => ft_core::Severity::Sev3,
            Self::Sev4 => ft_core::Severity::Sev4,
        }
    }
}

/// Risk-class selector (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum RiskClassArg {
    /// Security risk (high-stakes).
    Security,
    /// Availability risk (high-stakes).
    Availability,
    /// Data-loss risk (high-stakes).
    DataLoss,
    /// Compliance risk (high-stakes).
    Compliance,
    /// Performance risk.
    Performance,
    /// Correctness risk.
    Correctness,
}

impl RiskClassArg {
    /// Convert to `ft_core::RiskClass`.
    #[must_use]
    pub fn to_core(self) -> ft_core::RiskClass {
        match self {
            Self::Security => ft_core::RiskClass::Security,
            Self::Availability => ft_core::RiskClass::Availability,
            Self::DataLoss => ft_core::RiskClass::DataLoss,
            Self::Compliance => ft_core::RiskClass::Compliance,
            Self::Performance => ft_core::RiskClass::Performance,
            Self::Correctness => ft_core::RiskClass::Correctness,
        }
    }
}

/// Trust-state filter selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum TrustStateArg {
    /// Newly authored.
    Draft,
    /// Human-reviewed.
    Reviewed,
    /// Verified by two reviewers.
    Verified,
    /// Aged out.
    Stale,
    /// Deprecated.
    Deprecated,
    /// Archived.
    Archived,
    /// Superseded.
    Superseded,
    /// Rejected.
    Rejected,
    /// Redacted.
    Redacted,
}

impl TrustStateArg {
    /// Convert to `ft_core::TrustState`.
    #[must_use]
    pub fn to_core(self) -> ft_core::TrustState {
        match self {
            Self::Draft => ft_core::TrustState::Draft,
            Self::Reviewed => ft_core::TrustState::Reviewed,
            Self::Verified => ft_core::TrustState::Verified,
            Self::Stale => ft_core::TrustState::Stale,
            Self::Deprecated => ft_core::TrustState::Deprecated,
            Self::Archived => ft_core::TrustState::Archived,
            Self::Superseded => ft_core::TrustState::Superseded,
            Self::Rejected => ft_core::TrustState::Rejected,
            Self::Redacted => ft_core::TrustState::Redacted,
        }
    }
}

/// Evidence-kind selector for promotion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum EvidenceKindArg {
    /// Incident report.
    IncidentReport,
    /// Pull request.
    PullRequest,
    /// Commit.
    Commit,
    /// Dashboard.
    Dashboard,
    /// Log query.
    LogQuery,
    /// Test result.
    TestResult,
    /// Jira ticket.
    JiraTicket,
    /// Confluence page.
    ConfluencePage,
    /// Manual note.
    ManualNote,
}

impl EvidenceKindArg {
    /// Convert to `ft_core::EvidenceKind`.
    #[must_use]
    pub fn to_core(self) -> ft_core::EvidenceKind {
        match self {
            Self::IncidentReport => ft_core::EvidenceKind::IncidentReport,
            Self::PullRequest => ft_core::EvidenceKind::PullRequest,
            Self::Commit => ft_core::EvidenceKind::Commit,
            Self::Dashboard => ft_core::EvidenceKind::Dashboard,
            Self::LogQuery => ft_core::EvidenceKind::LogQuery,
            Self::TestResult => ft_core::EvidenceKind::TestResult,
            Self::JiraTicket => ft_core::EvidenceKind::JiraTicket,
            Self::ConfluencePage => ft_core::EvidenceKind::ConfluencePage,
            Self::ManualNote => ft_core::EvidenceKind::ManualNote,
        }
    }
}

/// Memory-kind selector for `capture` / `memory list --kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum MemoryKindArg {
    /// Incident.
    Incident,
    /// Finding.
    Finding,
    /// Runbook.
    Runbook,
    /// Decision.
    Decision,
    /// Gotcha.
    Gotcha,
    /// Generic memory note.
    Memory,
}

impl MemoryKindArg {
    /// Convert to `ft_core::RecordKind`.
    #[must_use]
    pub fn to_core(self) -> ft_core::RecordKind {
        match self {
            Self::Incident => ft_core::RecordKind::Incident,
            Self::Finding => ft_core::RecordKind::Finding,
            Self::Runbook => ft_core::RecordKind::Runbook,
            Self::Decision => ft_core::RecordKind::Decision,
            Self::Gotcha => ft_core::RecordKind::Gotcha,
            Self::Memory => ft_core::RecordKind::Memory,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Memory subcommands
// ─────────────────────────────────────────────────────────────────────────────

/// `firetrail incident …`
#[derive(Debug, Subcommand)]
pub enum IncidentCmd {
    /// Create a new incident.
    Create(CreateIncidentArgs),
}

/// `firetrail finding …`
#[derive(Debug, Subcommand)]
pub enum FindingCmd {
    /// Create a new finding.
    Create(CreateFindingArgs),
}

/// `firetrail runbook …`
#[derive(Debug, Subcommand)]
pub enum RunbookCmd {
    /// Create a new runbook.
    Create(CreateRunbookArgs),
    /// Step subcommands.
    #[command(subcommand)]
    Step(RunbookStepCmd),
}

/// `firetrail runbook step …`
#[derive(Debug, Subcommand)]
pub enum RunbookStepCmd {
    /// Add a step to an existing runbook.
    Add(RunbookStepAddArgs),
}

/// `firetrail decision …`
#[derive(Debug, Subcommand)]
pub enum DecisionCmd {
    /// Create a new decision.
    Create(CreateDecisionArgs),
}

/// `firetrail gotcha …`
#[derive(Debug, Subcommand)]
pub enum GotchaCmd {
    /// Create a new gotcha.
    Create(CreateGotchaArgs),
}

/// `firetrail memory …`
#[derive(Debug, Subcommand)]
pub enum MemoryCmd {
    /// Create a new generic memory note.
    Create(CreateMemoryArgs),
    /// List memory records.
    List(MemoryListArgs),
    /// List stale memory records.
    Stale(MemoryStaleArgs),
    /// Show a memory record with body rendering.
    Show(MemoryShowArgs),
    /// Promote Draft → Reviewed.
    Review(TrustReviewArgs),
    /// Promote Reviewed → Verified.
    Promote(TrustPromoteArgs),
    /// Mark a record Deprecated.
    Deprecate(TrustReasonArgs),
    /// Archive a record (terminal).
    Archive(TrustSimpleArgs),
    /// Supersede a record by another.
    Supersede(TrustSupersedeArgs),
    /// Merge multiple records into a canonical one.
    Merge(TrustMergeArgs),
    /// Redact a record (irreversible body wipe).
    Redact(TrustReasonArgs),
}

/// Incident creation arguments.
#[derive(Debug, Args)]
pub struct CreateIncidentArgs {
    /// One-line summary of what happened.
    pub summary: String,

    /// Severity classification.
    #[arg(long, value_enum)]
    pub severity: Option<SeverityArg>,

    /// RFC3339 timestamp the incident began (defaults to now).
    #[arg(long)]
    pub started_at: Option<String>,

    /// Comma-separated list of services affected.
    #[arg(long)]
    pub services: Option<String>,

    /// Risk classification (ADR-0013).
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// Finding creation arguments.
#[derive(Debug, Args)]
pub struct CreateFindingArgs {
    /// One-line summary.
    pub summary: String,

    /// Parent incident id, if any.
    #[arg(long)]
    pub incident: Option<String>,

    /// Long-form details (markdown).
    #[arg(long)]
    pub details: Option<String>,

    /// Risk classification.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,

    /// Comma-separated affected paths.
    #[arg(long)]
    pub affected: Option<String>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// Runbook creation arguments.
#[derive(Debug, Args)]
pub struct CreateRunbookArgs {
    /// Short title.
    pub title: String,

    /// One-line summary describing when to use the runbook.
    #[arg(long)]
    pub summary: String,

    /// Comma-separated `applies_to` service names.
    #[arg(long)]
    pub applies_to: Option<String>,

    /// Risk classification.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// `firetrail runbook step add` arguments.
#[derive(Debug, Args)]
pub struct RunbookStepAddArgs {
    /// Runbook record id.
    pub id: String,
    /// Step description.
    #[arg(long)]
    pub description: String,
    /// Shell command (or other invocation) for the step.
    #[arg(long)]
    pub command: Option<String>,
    /// What the operator should observe.
    #[arg(long)]
    pub expected: String,
}

/// Decision creation arguments.
#[derive(Debug, Args)]
pub struct CreateDecisionArgs {
    /// Short title.
    pub title: String,
    /// Background / problem statement.
    #[arg(long)]
    pub context: String,
    /// The decision itself.
    #[arg(long)]
    pub decision: String,
    /// Consequences of the decision.
    #[arg(long)]
    pub consequences: Option<String>,
    /// Risk classification.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,
    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// Gotcha creation arguments.
#[derive(Debug, Args)]
pub struct CreateGotchaArgs {
    /// One-line summary.
    pub summary: String,
    /// Long-form details (markdown).
    #[arg(long)]
    pub details: Option<String>,
    /// Risk classification.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,
    /// Comma-separated affected paths.
    #[arg(long)]
    pub affected: Option<String>,
    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// Memory note creation arguments.
#[derive(Debug, Args)]
pub struct CreateMemoryArgs {
    /// Short title.
    pub title: String,
    /// Markdown body (use `--body -` or `firetrail capture` for stdin input).
    #[arg(long)]
    pub body: String,
    /// Comma-separated tags.
    #[arg(long)]
    pub tags: Option<String>,
    /// Risk classification.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,
    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// `firetrail memory list` arguments.
#[derive(Debug, Args)]
pub struct MemoryListArgs {
    /// Restrict to a single memory kind.
    #[arg(long, value_enum)]
    pub kind: Option<MemoryKindArg>,
    /// Filter by trust state.
    #[arg(long, value_enum)]
    pub trust: Option<TrustStateArg>,
    /// Filter by risk class.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,
    /// Only show records whose freshness window has passed.
    #[arg(long)]
    pub stale: bool,
    /// Cap the number of results.
    #[arg(long)]
    pub limit: Option<u64>,
}

/// `firetrail memory stale` arguments.
#[derive(Debug, Args)]
pub struct MemoryStaleArgs {
    /// Restrict to a single memory kind.
    #[arg(long, value_enum)]
    pub kind: Option<MemoryKindArg>,
}

/// `firetrail memory show` arguments.
#[derive(Debug, Args)]
pub struct MemoryShowArgs {
    /// Record id (full or prefix).
    pub id: String,
}

/// `firetrail memory review` arguments.
#[derive(Debug, Args)]
pub struct TrustReviewArgs {
    /// Record id.
    pub id: String,
    /// Free-form reason / review summary.
    #[arg(long)]
    pub reason: Option<String>,
    /// URL pointing to supporting evidence.
    #[arg(long)]
    pub evidence_url: Option<String>,
}

/// `firetrail memory promote` arguments.
#[derive(Debug, Args)]
pub struct TrustPromoteArgs {
    /// Record id.
    pub id: String,
    /// Free-form reason.
    #[arg(long)]
    pub reason: Option<String>,
    /// URL pointing to supporting evidence.
    #[arg(long)]
    pub evidence_url: Option<String>,
    /// Evidence kind (defaults to `manual_note`).
    #[arg(long, value_enum)]
    pub evidence_type: Option<EvidenceKindArg>,
}

/// `firetrail memory deprecate` / `… redact` arguments.
#[derive(Debug, Args)]
pub struct TrustReasonArgs {
    /// Record id.
    pub id: String,
    /// Reason (required).
    #[arg(long)]
    pub reason: String,
}

/// `firetrail memory archive` arguments.
#[derive(Debug, Args)]
pub struct TrustSimpleArgs {
    /// Record id.
    pub id: String,
}

/// `firetrail memory supersede` arguments.
#[derive(Debug, Args)]
pub struct TrustSupersedeArgs {
    /// Record id being superseded.
    pub id: String,
    /// Successor record id.
    #[arg(long = "with")]
    pub successor: String,
    /// Optional reason.
    #[arg(long)]
    pub reason: Option<String>,
}

/// `firetrail memory merge` arguments.
#[derive(Debug, Args)]
pub struct TrustMergeArgs {
    /// Canonical record id (kept; others are superseded by it).
    pub canonical: String,
    /// Other record ids to fold into the canonical record.
    #[arg(required = true)]
    pub others: Vec<String>,
    /// Optional reason recorded on each supersede transition.
    #[arg(long)]
    pub reason: Option<String>,
}

/// `firetrail capture` arguments.
#[derive(Debug, Args)]
pub struct CaptureArgs {
    /// Memory kind (defaults to generic `memory`).
    #[arg(long, value_enum, default_value_t = MemoryKindArg::Memory)]
    pub kind: MemoryKindArg,
    /// Title / summary (required).
    #[arg(long)]
    pub title: String,
    /// Body content. If omitted, the body is read from stdin.
    #[arg(long)]
    pub body: Option<String>,
    /// Comma-separated tags.
    #[arg(long)]
    pub tags: Option<String>,
    /// Risk classification.
    #[arg(long, value_enum)]
    pub risk_class: Option<RiskClassArg>,
    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,
}

/// `firetrail verify` arguments.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// Specific record id to verify; verifies every record when omitted.
    pub id: Option<String>,
    /// Force walking every record (default when no id is provided).
    #[arg(long)]
    pub all: bool,
}

/// `firetrail compact` arguments.
#[derive(Debug, Args)]
pub struct CompactArgs {
    /// Compact a single record id.
    pub id: Option<String>,
    /// Compact every record changed between two git refs (`base..head`).
    #[arg(long, conflicts_with = "id")]
    pub pr: Option<String>,
}

/// `firetrail check …`
#[derive(Debug, Subcommand)]
pub enum CheckCmd {
    /// Validate the records changed between two git refs.
    Pr(CheckPrArgs),
}

/// `firetrail check pr` arguments.
#[derive(Debug, Args)]
pub struct CheckPrArgs {
    /// Base git ref of the PR.
    pub base: String,
    /// Head git ref of the PR.
    pub head: String,
}

/// Arguments for `firetrail init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Storage backend. M1 enforces embedded; `external` falls through with a
    /// warning. See ADR-0006.
    #[arg(long, value_enum, default_value_t = StorageModeArg::Embedded)]
    pub storage_mode: StorageModeArg,

    /// Reject identities not present in the registry. Persists as
    /// `identity.strict: true` in `config.yml`.
    #[arg(long)]
    pub strict_identity: bool,

    /// Skip writing `AGENTS.md` / `.claude/skills/firetrail/`.
    #[arg(long)]
    pub no_agents: bool,

    /// Skip installing git hooks.
    #[arg(long)]
    pub no_hooks: bool,
}

/// Arguments for `firetrail doctor`.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Run network checks (M5+). M1: no-op with a note.
    #[arg(long)]
    pub network: bool,

    /// Apply safe remediations for failed checks (rebuild index, reinstall hooks).
    #[arg(long)]
    pub fix: bool,
}

/// Storage-mode selection for `firetrail init`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StorageModeArg {
    /// JSON-in-Git records live under `.firetrail/records/` in the current repo.
    Embedded,
    /// Records live in a separate repository (M5; not yet available).
    External,
}

/// Priority selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum PriorityArg {
    /// Critical, top-of-queue.
    P0,
    /// High priority.
    P1,
    /// Normal priority.
    P2,
    /// Low priority.
    P3,
    /// Backlog.
    P4,
}

impl PriorityArg {
    /// Convert to `ft_core::Priority`.
    #[must_use]
    pub fn to_core(self) -> ft_core::Priority {
        match self {
            Self::P0 => ft_core::Priority::P0,
            Self::P1 => ft_core::Priority::P1,
            Self::P2 => ft_core::Priority::P2,
            Self::P3 => ft_core::Priority::P3,
            Self::P4 => ft_core::Priority::P4,
        }
    }
}

/// Workflow status selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum StatusArg {
    /// Open.
    Open,
    /// Ready (triaged).
    Ready,
    /// In progress.
    InProgress,
    /// In review.
    Review,
    /// Blocked.
    Blocked,
    /// Closed.
    Closed,
    /// Deferred.
    Deferred,
    /// Archived.
    Archived,
}

impl StatusArg {
    /// Convert to `ft_core::Status`.
    #[must_use]
    pub fn to_core(self) -> ft_core::Status {
        match self {
            Self::Open => ft_core::Status::Open,
            Self::Ready => ft_core::Status::Ready,
            Self::InProgress => ft_core::Status::InProgress,
            Self::Review => ft_core::Status::Review,
            Self::Blocked => ft_core::Status::Blocked,
            Self::Closed => ft_core::Status::Closed,
            Self::Deferred => ft_core::Status::Deferred,
            Self::Archived => ft_core::Status::Archived,
        }
    }
}

/// Record-kind selector for filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum KindArg {
    /// Epic.
    Epic,
    /// Task.
    Task,
    /// Subtask.
    Subtask,
    /// Bug.
    Bug,
}

impl KindArg {
    /// Convert to `ft_core::RecordKind`.
    #[must_use]
    pub fn to_core(self) -> ft_core::RecordKind {
        match self {
            Self::Epic => ft_core::RecordKind::Epic,
            Self::Task => ft_core::RecordKind::Task,
            Self::Subtask => ft_core::RecordKind::Subtask,
            Self::Bug => ft_core::RecordKind::Bug,
        }
    }
}

/// Relation-kind selector for `link` / `dep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum RelationKindArg {
    /// blocks
    Blocks,
    /// blocked-by
    BlockedBy,
    /// parent-of
    ParentOf,
    /// child-of
    ChildOf,
    /// related-to
    RelatedTo,
    /// duplicates
    Duplicates,
    /// supersedes
    Supersedes,
    /// fixed-by
    FixedBy,
    /// caused-by
    CausedBy,
}

impl RelationKindArg {
    /// Convert to `ft_core::RelationKind`.
    #[must_use]
    pub fn to_core(self) -> ft_core::RelationKind {
        match self {
            Self::Blocks => ft_core::RelationKind::Blocks,
            Self::BlockedBy => ft_core::RelationKind::BlockedBy,
            Self::ParentOf => ft_core::RelationKind::ParentOf,
            Self::ChildOf => ft_core::RelationKind::ChildOf,
            Self::RelatedTo => ft_core::RelationKind::RelatedTo,
            Self::Duplicates => ft_core::RelationKind::Duplicates,
            Self::Supersedes => ft_core::RelationKind::Supersedes,
            Self::FixedBy => ft_core::RelationKind::FixedBy,
            Self::CausedBy => ft_core::RelationKind::CausedBy,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-command argument groups
// ─────────────────────────────────────────────────────────────────────────────

/// `firetrail epic …`
#[derive(Debug, Subcommand)]
pub enum EpicCmd {
    /// Create a new epic.
    Create(CreateRecordArgs),
}

/// `firetrail task …`
#[derive(Debug, Subcommand)]
pub enum TaskCmd {
    /// Create a new task.
    Create(CreateTaskArgs),
}

/// `firetrail subtask …`
#[derive(Debug, Subcommand)]
pub enum SubtaskCmd {
    /// Create a new subtask under a parent task.
    Create(CreateSubtaskArgs),
}

/// `firetrail bug …`
#[derive(Debug, Subcommand)]
pub enum BugCmd {
    /// Create a new bug.
    Create(CreateBugArgs),
}

/// Common arguments for record-creation commands without kind-specific fields.
#[derive(Debug, Args)]
pub struct CreateRecordArgs {
    /// Title (required).
    pub title: String,

    /// Free-form description.
    #[arg(long)]
    pub description: Option<String>,

    /// Priority.
    #[arg(long, value_enum)]
    pub priority: Option<PriorityArg>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,

    /// Free-form `key=value` label. May be repeated.
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,
}

/// Task-specific creation arguments.
#[derive(Debug, Args)]
pub struct CreateTaskArgs {
    /// Title (required).
    pub title: String,

    /// Free-form description.
    #[arg(long)]
    pub description: Option<String>,

    /// Parent epic id (full id or unambiguous prefix).
    #[arg(long)]
    pub epic: Option<String>,

    /// Priority.
    #[arg(long, value_enum)]
    pub priority: Option<PriorityArg>,

    /// Owner identity.
    #[arg(long)]
    pub owner: Option<String>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,

    /// Free-form `key=value` label. May be repeated.
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,
}

/// Subtask-specific creation arguments.
#[derive(Debug, Args)]
pub struct CreateSubtaskArgs {
    /// Title (required).
    pub title: String,

    /// Parent task id (required, full id or unambiguous prefix).
    #[arg(long)]
    pub parent: String,

    /// Free-form description.
    #[arg(long)]
    pub description: Option<String>,

    /// Priority.
    #[arg(long, value_enum)]
    pub priority: Option<PriorityArg>,

    /// Owner identity.
    #[arg(long)]
    pub owner: Option<String>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,

    /// Free-form `key=value` label. May be repeated.
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,
}

/// Bug-specific creation arguments.
#[derive(Debug, Args)]
pub struct CreateBugArgs {
    /// Title (required).
    pub title: String,

    /// Free-form description.
    #[arg(long)]
    pub description: Option<String>,

    /// Affected service.
    #[arg(long)]
    pub service: Option<String>,

    /// Severity (`sev1`, `sev2`, `sev3` — free-form at M1).
    #[arg(long)]
    pub severity: Option<String>,

    /// Priority.
    #[arg(long, value_enum)]
    pub priority: Option<PriorityArg>,

    /// Owning scope.
    #[arg(long)]
    pub scope: Option<String>,

    /// Free-form `key=value` label. May be repeated.
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,
}

/// `firetrail update <id> [...flags]`
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Record id (full or unambiguous prefix).
    pub id: String,

    /// New title.
    #[arg(long)]
    pub title: Option<String>,

    /// New status.
    #[arg(long, value_enum)]
    pub status: Option<StatusArg>,

    /// New priority.
    #[arg(long, value_enum)]
    pub priority: Option<PriorityArg>,

    /// New owner identity.
    #[arg(long)]
    pub owner: Option<String>,
}

/// `firetrail close <id> [--force --reason <text>]`
#[derive(Debug, Args)]
pub struct CloseArgs {
    /// Record id.
    pub id: String,

    /// Skip acceptance-criteria validation.
    #[arg(long, requires = "reason")]
    pub force: bool,

    /// Reason for forcing close. Required when `--force` is supplied.
    #[arg(long)]
    pub reason: Option<String>,
}

/// `firetrail reopen <id>`
#[derive(Debug, Args)]
pub struct ReopenArgs {
    /// Record id.
    pub id: String,
}

/// `firetrail claim <id> [--expires <duration>]`
#[derive(Debug, Args)]
pub struct ClaimArgs {
    /// Record id.
    pub id: String,

    /// Human-readable duration override (e.g. `7d`, `12h`).
    #[arg(long)]
    pub expires: Option<String>,
}

/// `firetrail unclaim <id>`
#[derive(Debug, Args)]
pub struct UnclaimArgs {
    /// Record id.
    pub id: String,

    /// Take over another actor's claim (M5; M1 refuses).
    #[arg(long)]
    pub takeover: bool,

    /// Reason for takeover. Required with `--takeover`.
    #[arg(long)]
    pub reason: Option<String>,
}

/// `firetrail criteria …`
#[derive(Debug, Subcommand)]
pub enum CriteriaCmd {
    /// Add a new acceptance criterion.
    Add(CriteriaAddArgs),
    /// List all criteria for a record.
    List(CriteriaListArgs),
    /// Mark a criterion as checked.
    Check(CriteriaToggleArgs),
    /// Mark a criterion as unchecked.
    Uncheck(CriteriaToggleArgs),
    /// Attach an evidence URL to a criterion.
    Evidence(CriteriaEvidenceArgs),
}

/// Arguments for `firetrail criteria add`.
#[derive(Debug, Args)]
pub struct CriteriaAddArgs {
    /// Record id.
    pub id: String,
    /// Criterion text.
    pub text: String,
}

/// Arguments for `firetrail criteria list`.
#[derive(Debug, Args)]
pub struct CriteriaListArgs {
    /// Record id.
    pub id: String,
}

/// Arguments for `firetrail criteria {check,uncheck}`.
#[derive(Debug, Args)]
pub struct CriteriaToggleArgs {
    /// Record id.
    pub id: String,
    /// AC id (`ac-02`) or 1-based index.
    pub which: String,
}

/// Arguments for `firetrail criteria evidence`.
#[derive(Debug, Args)]
pub struct CriteriaEvidenceArgs {
    /// Record id.
    pub id: String,
    /// AC id or 1-based index.
    pub which: String,
    /// Evidence URL.
    #[arg(long)]
    pub url: String,
}

/// `firetrail link <from> <to> --type <kind>`
#[derive(Debug, Args)]
pub struct LinkArgs {
    /// Source record id.
    pub from: String,
    /// Target record id.
    pub to: String,
    /// Relation kind.
    #[arg(long = "type", value_enum)]
    pub kind: RelationKindArg,
}

/// `firetrail dep …`
#[derive(Debug, Subcommand)]
pub enum DepCmd {
    /// Add a dependency edge.
    Add(DepAddArgs),
    /// Remove a dependency edge.
    Remove(DepRemoveArgs),
}

/// Arguments for `firetrail dep add`.
#[derive(Debug, Args)]
pub struct DepAddArgs {
    /// Source record id.
    pub from: String,
    /// Target record id.
    pub to: String,
    /// Relation kind (defaults to `blocked-by`).
    #[arg(long = "type", value_enum, default_value_t = RelationKindArg::BlockedBy)]
    pub kind: RelationKindArg,
}

/// Arguments for `firetrail dep remove`.
#[derive(Debug, Args)]
pub struct DepRemoveArgs {
    /// Source record id.
    pub from: String,
    /// Target record id.
    pub to: String,
    /// Specific relation kind to remove (optional; removes all matching when omitted).
    #[arg(long = "type", value_enum)]
    pub kind: Option<RelationKindArg>,
}

/// `firetrail show <id>`
#[derive(Debug, Args)]
pub struct ShowArgs {
    /// Record id.
    pub id: String,
}

/// `firetrail list …`
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Filter by kind.
    #[arg(long = "type", value_enum)]
    pub kind: Option<KindArg>,
    /// Filter by status.
    #[arg(long, value_enum)]
    pub status: Option<StatusArg>,
    /// Filter by owner.
    #[arg(long)]
    pub owner: Option<String>,
    /// Filter by scope.
    #[arg(long)]
    pub scope: Option<String>,
    /// Cap the number of results.
    #[arg(long)]
    pub limit: Option<u64>,
    /// Skip the first N results.
    #[arg(long)]
    pub offset: Option<u64>,
}

/// `firetrail ready …`
#[derive(Debug, Args)]
pub struct ReadyArgs {
    /// Filter by kind.
    #[arg(long = "type", value_enum)]
    pub kind: Option<KindArg>,
    /// Filter by owner.
    #[arg(long)]
    pub owner: Option<String>,
    /// Filter by scope.
    #[arg(long)]
    pub scope: Option<String>,
    /// Cap the number of results.
    #[arg(long)]
    pub limit: Option<u64>,
}

/// `firetrail board …`
#[derive(Debug, Args)]
pub struct BoardArgs {
    /// Filter by scope.
    #[arg(long)]
    pub scope: Option<String>,
    /// Filter by owner.
    #[arg(long)]
    pub owner: Option<String>,
}

/// Walk direction for `firetrail graph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum GraphDirArg {
    /// Walk upstream blockers.
    Up,
    /// Walk downstream dependents.
    Down,
    /// Both directions.
    Both,
}

/// `firetrail graph <id>`
#[derive(Debug, Args)]
pub struct GraphArgs {
    /// Root record id.
    pub id: String,
    /// Walk direction (default: both).
    #[arg(long, value_enum, default_value_t = GraphDirArg::Both)]
    pub direction: GraphDirArg,
    /// Walk depth (default: 3).
    #[arg(long, default_value_t = 3)]
    pub depth: u32,
}
