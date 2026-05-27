//! Command handlers.
//!
//! Every subcommand returns a [`CommandOutcome`]. The outcome carries both a
//! markdown rendering and the JSON shape so that the [`crate::output`]
//! formatter can pick the right representation without command handlers
//! needing to know which format is selected.
//!
//! Adding a new subcommand:
//!
//! 1. Add a variant to [`crate::cli::Command`] with its `Args` struct.
//! 2. Add a `pub mod foo;` here and implement
//!    `pub fn run(args: &FooArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError>`.
//! 3. Wire the new variant into the `match` in `main::dispatch`.
//!
//! No other plumbing should be necessary — output formatting, error handling,
//! and exit codes are handled by the shared scaffold.

pub mod doctor;
pub mod init;

use serde_json::Value;

/// Outcome of a successful subcommand.
///
/// Each variant knows how to render itself as markdown and how to expose its
/// JSON `data` payload. Variants are added as new commands land; existing
/// callers (notably [`crate::output::Formatter`]) only see the public
/// interface, so adding variants is non-breaking.
#[derive(Debug, Clone)]
pub enum CommandOutcome {
    /// The result of `firetrail init`.
    Init(init::InitReport),
    /// The result of `firetrail doctor`.
    Doctor(doctor::DoctorReport),
}

impl CommandOutcome {
    /// Stable command name (used for the JSON envelope's `command` field).
    #[must_use]
    pub fn command(&self) -> &'static str {
        match self {
            CommandOutcome::Init(_) => "init",
            CommandOutcome::Doctor(_) => "doctor",
        }
    }

    /// Markdown rendering for human consumption.
    #[must_use]
    pub fn markdown(&self) -> String {
        match self {
            CommandOutcome::Init(r) => r.markdown(),
            CommandOutcome::Doctor(r) => r.markdown(),
        }
    }

    /// One-line markdown used in `--quiet` mode.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        match self {
            CommandOutcome::Init(r) => r.quiet_line(),
            CommandOutcome::Doctor(r) => r.quiet_line(),
        }
    }

    /// JSON payload (placed under `data` in the envelope).
    #[must_use]
    pub fn json_data(&self) -> Value {
        match self {
            CommandOutcome::Init(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            CommandOutcome::Doctor(r) => serde_json::to_value(r).unwrap_or(Value::Null),
        }
    }

    /// Non-fatal warnings to surface in the JSON envelope.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        match self {
            CommandOutcome::Init(r) => r.warnings.clone(),
            CommandOutcome::Doctor(r) => r.warnings.clone(),
        }
    }
}
