//! # firetrail
//!
//! The `firetrail` binary — a `clap`-based dispatcher over the workspace
//! crates. M1 ships the scaffold plus `init` and `doctor`; subsequent epics
//! plug in additional subcommands using the [`CommandOutcome`] contract and
//! the [`output`] formatter.
//!
//! ## Relevant ADRs
//!
//! - ADR-0001 — Rust as the implementation language
//! - ADR-0011 — Offline-first contract
//! - ADR-0016 — Build approach

#![deny(missing_docs)]

use std::process::ExitCode;

use clap::Parser;

mod cli;
mod commands;
mod error;
mod output;
mod workspace;

use crate::cli::{Cli, Command};
use crate::error::CliError;
use crate::output::{Formatter, OutputFormat};

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialise tracing if --verbose was passed; honour RUST_LOG otherwise.
    if cli.global.verbose {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .with_writer(std::io::stderr)
            .try_init();
    }

    let format = OutputFormat::resolve(cli.global.format, cli.global.json);
    let formatter = Formatter::new(format, cli.global.quiet);

    let started = std::time::Instant::now();
    let outcome = dispatch(&cli);
    let elapsed_ms = u128::min(started.elapsed().as_millis(), u128::from(u64::MAX));
    let elapsed_ms = u64::try_from(elapsed_ms).unwrap_or(u64::MAX);

    match outcome {
        Ok(out) => {
            formatter.render_ok(out.command(), &out, elapsed_ms);
            ExitCode::from(0)
        }
        Err(err) => {
            let code = err.exit_code();
            formatter.render_err(err.command(), &err, elapsed_ms);
            ExitCode::from(code)
        }
    }
}

fn dispatch(cli: &Cli) -> Result<commands::CommandOutcome, CliError> {
    match &cli.command {
        Command::Init(args) => commands::init::run(args, &cli.global),
        Command::Doctor(args) => commands::doctor::run(args, &cli.global),
    }
}
