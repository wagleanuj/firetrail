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
mod context;
mod error;
mod output;
mod prompt;
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

#[allow(clippy::too_many_lines)]
fn dispatch(cli: &Cli) -> Result<commands::CommandOutcome, CliError> {
    use crate::cli::{
        BugCmd, CheckCmd, CriteriaCmd, DaemonCmd, DecisionCmd, DepCmd, DocCmd, EpicCmd, FindingCmd,
        GotchaCmd, HookCmd, IdentityCmd, ImportCmd, IncidentCmd, IndexCmd, LintCmd, MemoryCmd,
        MigrateCmd, RunbookCmd, RunbookStepCmd, ScopeCmd, ServerHooksCmd, SubtaskCmd, TaskCmd,
    };
    match &cli.command {
        Command::Init(args) => commands::init::run(args, &cli.global),
        Command::Doctor(args) => commands::doctor::run(args, &cli.global),
        Command::Epic(EpicCmd::Create(args)) => commands::create::epic(args, &cli.global),
        Command::Task(TaskCmd::Create(args)) => commands::create::task(args, &cli.global),
        Command::Subtask(SubtaskCmd::Create(args)) => commands::create::subtask(args, &cli.global),
        Command::Bug(BugCmd::Create(args)) => commands::create::bug(args, &cli.global),
        Command::Update(args) => commands::update::run(args, &cli.global),
        Command::Close(args) => commands::close::close(args, &cli.global),
        Command::Reopen(args) => commands::close::reopen(args, &cli.global),
        Command::Claim(args) => commands::claim::claim(args, &cli.global),
        Command::Unclaim(args) => commands::claim::unclaim(args, &cli.global),
        Command::Criteria(CriteriaCmd::Add(args)) => commands::criteria::add(args, &cli.global),
        Command::Criteria(CriteriaCmd::List(args)) => commands::criteria::list(args, &cli.global),
        Command::Criteria(CriteriaCmd::Check(args)) => commands::criteria::check(args, &cli.global),
        Command::Criteria(CriteriaCmd::Uncheck(args)) => {
            commands::criteria::uncheck(args, &cli.global)
        }
        Command::Criteria(CriteriaCmd::Evidence(args)) => {
            commands::criteria::evidence(args, &cli.global)
        }
        Command::Link(args) => commands::link::link(args, &cli.global),
        Command::Dep(DepCmd::Add(args)) => commands::link::dep_add(args, &cli.global),
        Command::Dep(DepCmd::Remove(args)) => commands::link::dep_remove(args, &cli.global),
        Command::Show(args) => commands::show::run(args, &cli.global),
        Command::List(args) => commands::list::list(args, &cli.global),
        Command::Ready(args) => commands::list::ready(args, &cli.global),
        Command::Board(args) => commands::board::run(args, &cli.global),
        Command::Graph(args) => commands::graph::run(args, &cli.global),

        Command::Incident(IncidentCmd::Create(args)) => {
            commands::memory_create::incident(args, &cli.global)
        }
        Command::Finding(FindingCmd::Create(args)) => {
            commands::memory_create::finding(args, &cli.global)
        }
        Command::Runbook(RunbookCmd::Create(args)) => {
            commands::memory_create::runbook(args, &cli.global)
        }
        Command::Runbook(RunbookCmd::Step(RunbookStepCmd::Add(args))) => {
            commands::trust::runbook_step_add(args, &cli.global)
        }
        Command::Decision(DecisionCmd::Create(args)) => {
            commands::memory_create::decision(args, &cli.global)
        }
        Command::Gotcha(GotchaCmd::Create(args)) => {
            commands::memory_create::gotcha(args, &cli.global)
        }

        Command::Memory(MemoryCmd::Create(args)) => {
            commands::memory_create::memory(args, &cli.global)
        }
        Command::Memory(MemoryCmd::List(args)) => commands::memory_views::list(args, &cli.global),
        Command::Memory(MemoryCmd::Stale(args)) => commands::memory_views::stale(args, &cli.global),
        Command::Memory(MemoryCmd::Show(args)) => commands::memory_views::show(args, &cli.global),
        Command::Memory(MemoryCmd::Review(args)) => commands::trust::review(args, &cli.global),
        Command::Memory(MemoryCmd::Promote(args)) => commands::trust::promote(args, &cli.global),
        Command::Memory(MemoryCmd::Deprecate(args)) => {
            commands::trust::deprecate(args, &cli.global)
        }
        Command::Memory(MemoryCmd::Archive(args)) => commands::trust::archive(args, &cli.global),
        Command::Memory(MemoryCmd::Supersede(args)) => {
            commands::trust::supersede(args, &cli.global)
        }
        Command::Memory(MemoryCmd::Merge(args)) => commands::trust::merge(args, &cli.global),
        Command::Memory(MemoryCmd::Redact(args)) => commands::trust::redact(args, &cli.global),
        Command::Memory(MemoryCmd::Salvage(args)) => commands::salvage::run(args, &cli.global),

        Command::Capture(args) => commands::memory_create::capture(args, &cli.global),
        Command::Verify(args) => commands::verify::run(args, &cli.global),
        Command::Compact(args) => commands::compact::run(args, &cli.global),
        Command::Check(CheckCmd::Pr(args)) => commands::check::pr(args, &cli.global),
        Command::Check(CheckCmd::Paths(args)) => commands::check::paths(args, &cli.global),
        Command::Diff(args) => commands::diff::run(args, &cli.global),
        Command::Lint(LintCmd::Memory(args)) => commands::lint::memory(args, &cli.global),
        Command::Review(args) => commands::review::run(args, &cli.global),
        Command::MergeDriverInstall(args) => commands::merge_driver::install(args, &cli.global),
        Command::ServerHooks(ServerHooksCmd::Install(args)) => {
            commands::server_hooks::install(args, &cli.global)
        }

        Command::Hook(HookCmd::OnCheckout(args)) => commands::hook::on_checkout(args, &cli.global),
        Command::Hook(HookCmd::OnMerge(args)) => commands::hook::on_merge(args, &cli.global),

        Command::Search(args) => commands::search::search(args, &cli.global),
        Command::Similar(args) => commands::search::similar(args, &cli.global),
        Command::Prime(args) => commands::prime::run(args, &cli.global),
        Command::Index(IndexCmd::Rebuild) => commands::index_cmd::rebuild(&cli.global),
        Command::Index(IndexCmd::Refresh) => commands::index_cmd::refresh(&cli.global),
        Command::Daemon(DaemonCmd::Start(args)) => commands::daemon_cmd::start(args, &cli.global),
        Command::Daemon(DaemonCmd::Stop) => commands::daemon_cmd::stop(&cli.global),
        Command::Daemon(DaemonCmd::Status) => commands::daemon_cmd::status(&cli.global),
        Command::Ui(args) => commands::ui::run(args, &cli.global),

        Command::ClaimTakeover(args) => commands::claim::takeover(args, &cli.global),

        Command::Identity(IdentityCmd::Register(args)) => {
            commands::identity::register(args, &cli.global)
        }
        Command::Identity(IdentityCmd::List(args)) => commands::identity::list(args, &cli.global),
        Command::Identity(IdentityCmd::Show(args)) => commands::identity::show(args, &cli.global),
        Command::Identity(IdentityCmd::Offboard(args)) => {
            commands::identity::offboard(args, &cli.global)
        }
        Command::Doc(DocCmd::Add(args)) => commands::doc::add(args, &cli.global),
        Command::Doc(DocCmd::Link(args)) => commands::doc::link(args, &cli.global),
        Command::Doc(DocCmd::Index(args)) => commands::doc::index(args, &cli.global),

        Command::Scope(ScopeCmd::List) => commands::scope::list(&cli.global),
        Command::Scope(ScopeCmd::Show(args)) => commands::scope::show(args, &cli.global),
        Command::Scope(ScopeCmd::Aliases) => commands::scope::aliases(&cli.global),
        Command::Scope(ScopeCmd::Owners(args)) => commands::scope::owners(args, &cli.global),

        Command::Sync(args) => commands::sync_cmd::run(args, &cli.global),

        Command::Import(ImportCmd::Incidents(args)) => {
            commands::import_cmd::incidents(args, &cli.global)
        }
        Command::Import(ImportCmd::Adrs(args)) => commands::import_cmd::adrs(args, &cli.global),
        Command::Import(ImportCmd::Runbooks(args)) => {
            commands::import_cmd::runbooks(args, &cli.global)
        }
        Command::Import(ImportCmd::Refresh(args)) => {
            commands::import_cmd::refresh(args, &cli.global)
        }
        Command::PromoteImport(args) => commands::promote_import::run(args, &cli.global),
        Command::Migrate(MigrateCmd::Embeddings(args)) => {
            commands::migrate::embeddings(args, &cli.global)
        }
    }
}
