//! `cargo xtask` — repo automation entrypoint.
//!
//! Today this only exports TypeScript bindings for `ft-ops`'s wire types into
//! `crates/ft-ui/web/src/api/types/` via [`ts-rs`]. Two subcommands:
//!
//! - `gen-ts`   — (re)generate the committed bindings in place.
//! - `check-ts` — generate into a tempdir and diff against the committed copy;
//!   exits 1 on any drift. CI runs this to keep wire types honest.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use ft_ops::memory::{
    CaptureInput, CreateDecisionInput, CreateFindingInput, CreateGotchaInput,
    CreateIncidentInput, CreateMemoryInput, CreateRunbookInput, ListInput as MemoryListInput,
    ListOutput as MemoryListOutput, MemoryKind, MemoryRowOut, RiskClassInput, SalvageEntry,
    SalvageEntryAction, SalvageInput, SalvageOutput, SearchHitOut, SearchInput, SearchMode,
    SearchOutput, SeverityInput, ShowInput as MemoryShowInput, SimilarInput, StaleInput,
    TrustStateInput,
};
use ft_ops::tickets::{
    BoardCard, BoardInput, BoardOutput, ClaimInput, CloseInput, CreateBugInput, CreateEpicInput,
    CreateSubtaskInput, CreateTaskInput, LinkInput, ListInput, ListedTicket, ShowInput,
    TicketKindFilter, TicketPriority, TicketRelationKind, TicketStatusFilter, UnclaimInput,
    UpdateInput,
};
use ft_ops::events::SalvageDecision;
use ft_ops::{EmittedEvent, Event};
use ts_rs::TS;

/// Repo-internal automation tool.
#[derive(Debug, Parser)]
#[command(name = "xtask", about = "firetrail repo automation")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Generate TypeScript bindings for ft-ops types into the web/api/types/ dir.
    GenTs,
    /// Verify committed bindings are up to date. Exits 1 on any drift.
    CheckTs,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::GenTs => match run_gen_ts() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("gen-ts failed: {e:?}");
                ExitCode::FAILURE
            }
        },
        Cmd::CheckTs => match run_check_ts() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("check-ts failed: {e:?}");
                ExitCode::FAILURE
            }
        },
    }
}

fn types_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points at xtask/, repo root is its parent.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(manifest)
        .join("crates")
        .join("ft-ui")
        .join("web")
        .join("src")
        .join("api")
        .join("types")
}

fn run_gen_ts() -> Result<()> {
    let dir = types_dir();
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    export_into(&dir)?;
    println!("wrote ts bindings to {}", dir.display());
    Ok(())
}

fn run_check_ts() -> Result<()> {
    let committed = types_dir();
    let tmp = tempfile::tempdir().context("create tempdir for ts check")?;
    let tmp_dir = tmp.path().to_path_buf();
    export_into(&tmp_dir)?;
    diff_dirs(&committed, &tmp_dir)
}

/// Export every TS-tagged type from `ft-ops` into `dir`.
///
/// Add new types here as they grow a `#[cfg_attr(feature = "ts-rs",
/// derive(ts_rs::TS))]`. ts-rs 9 writes each top-level type to its own file
/// when called via [`TS::export_all_to`], which also pulls in transitive
/// dependencies (e.g. `Event` is exported when `EmittedEvent` is).
fn export_into(dir: &Path) -> Result<()> {
    Event::export_all_to(dir).context("export Event")?;
    EmittedEvent::export_all_to(dir).context("export EmittedEvent")?;
    // Ticket ops Inputs/Outputs (W1-A).
    BoardInput::export_all_to(dir).context("export BoardInput")?;
    BoardOutput::export_all_to(dir).context("export BoardOutput")?;
    BoardCard::export_all_to(dir).context("export BoardCard")?;
    ClaimInput::export_all_to(dir).context("export ClaimInput")?;
    UnclaimInput::export_all_to(dir).context("export UnclaimInput")?;
    CloseInput::export_all_to(dir).context("export CloseInput")?;
    CreateEpicInput::export_all_to(dir).context("export CreateEpicInput")?;
    CreateTaskInput::export_all_to(dir).context("export CreateTaskInput")?;
    CreateSubtaskInput::export_all_to(dir).context("export CreateSubtaskInput")?;
    CreateBugInput::export_all_to(dir).context("export CreateBugInput")?;
    TicketPriority::export_all_to(dir).context("export TicketPriority")?;
    LinkInput::export_all_to(dir).context("export LinkInput")?;
    TicketRelationKind::export_all_to(dir).context("export TicketRelationKind")?;
    ListInput::export_all_to(dir).context("export ListInput")?;
    ListedTicket::export_all_to(dir).context("export ListedTicket")?;
    TicketKindFilter::export_all_to(dir).context("export TicketKindFilter")?;
    TicketStatusFilter::export_all_to(dir).context("export TicketStatusFilter")?;
    ShowInput::export_all_to(dir).context("export ShowInput")?;
    UpdateInput::export_all_to(dir).context("export UpdateInput")?;
    // Memory ops Inputs/Outputs (W2-A).
    SalvageDecision::export_all_to(dir).context("export SalvageDecision")?;
    SeverityInput::export_all_to(dir).context("export SeverityInput")?;
    RiskClassInput::export_all_to(dir).context("export RiskClassInput")?;
    TrustStateInput::export_all_to(dir).context("export TrustStateInput")?;
    MemoryKind::export_all_to(dir).context("export MemoryKind")?;
    CreateIncidentInput::export_all_to(dir).context("export CreateIncidentInput")?;
    CreateFindingInput::export_all_to(dir).context("export CreateFindingInput")?;
    CreateRunbookInput::export_all_to(dir).context("export CreateRunbookInput")?;
    CreateDecisionInput::export_all_to(dir).context("export CreateDecisionInput")?;
    CreateGotchaInput::export_all_to(dir).context("export CreateGotchaInput")?;
    CreateMemoryInput::export_all_to(dir).context("export CreateMemoryInput")?;
    CaptureInput::export_all_to(dir).context("export CaptureInput")?;
    MemoryListInput::export_all_to(dir).context("export memory ListInput")?;
    MemoryListOutput::export_all_to(dir).context("export memory ListOutput")?;
    MemoryRowOut::export_all_to(dir).context("export MemoryRowOut")?;
    StaleInput::export_all_to(dir).context("export StaleInput")?;
    MemoryShowInput::export_all_to(dir).context("export memory ShowInput")?;
    SearchMode::export_all_to(dir).context("export SearchMode")?;
    SearchInput::export_all_to(dir).context("export SearchInput")?;
    SimilarInput::export_all_to(dir).context("export SimilarInput")?;
    SearchHitOut::export_all_to(dir).context("export SearchHitOut")?;
    SearchOutput::export_all_to(dir).context("export SearchOutput")?;
    SalvageEntryAction::export_all_to(dir).context("export SalvageEntryAction")?;
    SalvageEntry::export_all_to(dir).context("export SalvageEntry")?;
    SalvageInput::export_all_to(dir).context("export SalvageInput")?;
    SalvageOutput::export_all_to(dir).context("export SalvageOutput")?;
    Ok(())
}

fn diff_dirs(committed: &Path, generated: &Path) -> Result<()> {
    use std::collections::BTreeMap;

    let committed_files = read_ts_files(committed)?;
    let generated_files = read_ts_files(generated)?;

    let mut missing: Vec<String> = Vec::new();
    let mut extra: Vec<String> = Vec::new();
    let mut changed: Vec<String> = Vec::new();

    let all_names: BTreeMap<&String, ()> = committed_files
        .keys()
        .chain(generated_files.keys())
        .map(|k| (k, ()))
        .collect();

    for name in all_names.keys() {
        match (committed_files.get(*name), generated_files.get(*name)) {
            (Some(_), None) => extra.push((*name).clone()),
            (None, Some(_)) => missing.push((*name).clone()),
            (Some(a), Some(b)) if a != b => changed.push((*name).clone()),
            _ => {}
        }
    }

    if missing.is_empty() && extra.is_empty() && changed.is_empty() {
        println!("ts bindings up to date ({} files)", committed_files.len());
        return Ok(());
    }

    if !missing.is_empty() {
        eprintln!("missing in {}: {:?}", committed.display(), missing);
    }
    if !extra.is_empty() {
        eprintln!(
            "extra in {} (should not exist): {:?}",
            committed.display(),
            extra,
        );
    }
    if !changed.is_empty() {
        eprintln!("content drift: {:?}", changed);
    }
    bail!("run `cargo xtask gen-ts` and commit the result")
}

fn read_ts_files(dir: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    let mut out = std::collections::BTreeMap::new();
    let entries = fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        if !name.ends_with(".ts") {
            // Keep the .gitkeep / READMEs ignored.
            continue;
        }
        let content = fs::read_to_string(&path)?;
        out.insert(name, content);
    }
    Ok(out)
}
