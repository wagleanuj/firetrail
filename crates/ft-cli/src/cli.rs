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
///
/// The work-graph epic (firetrail-1xc) appends additional variants here; the
/// dispatcher in `main.rs` and the [`crate::commands::CommandOutcome`] enum
/// are the only other places that need to learn about new commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialise a Firetrail workspace in the current git repo.
    Init(InitArgs),

    /// Verify the workspace is healthy and report any actionable issues.
    Doctor(DoctorArgs),
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
