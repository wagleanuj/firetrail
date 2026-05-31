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
use ft_ops::audit::{
    CriteriaAddInput, CriteriaEvidenceInput, CriteriaListInput, CriteriaListOutput,
    CriteriaListRow, CriteriaToggleInput, DiffChange, DiffInput, DiffOutput, DiffRow,
    GraphDirectionInput, GraphEdge, GraphInput, GraphNode, GraphOutput, LintFinding, LintInput,
    LintOutput, LintSeverity, ReviewAcRow, ReviewEvidenceRow, ReviewHistoryRow,
    ReviewInput as AuditReviewInput, ReviewOutput as AuditReviewOutput, VerifyInput, VerifyOutput,
    VerifyResult,
};
use ft_ops::docs::{DocFreshnessView, DocView};
use ft_ops::events::SalvageDecision;
use ft_ops::identity_ops::{
    CapabilitiesInput, CapabilitiesOutput, CapabilityRow, IdentityKindInput, IdentityListOutput,
    IdentityRegisterOutput, IdentityShowOutput, IdentityStatusFilter, IdentityView,
    ListInput as IdentityListInput, OffboardInput, RegisterInput, ShowInput as IdentityShowInput,
};
use ft_ops::memory::{
    CaptureInput, CreateDecisionInput, CreateFindingInput, CreateGotchaInput, CreateIncidentInput,
    CreateMemoryInput, CreateRunbookInput, ListInput as MemoryListInput,
    ListOutput as MemoryListOutput, MemoryKind, MemoryRowOut, RiskClassInput, SalvageEntry,
    SalvageEntryAction, SalvageInput, SalvageOutput, SearchHitOut, SearchInput, SearchMode,
    SearchOutput, SeverityInput, ShowInput as MemoryShowInput, SimilarInput, StaleInput,
    TrustStateInput,
};
use ft_ops::scope::{
    AliasEntry, AliasesInput, AliasesOutput, CodeOwnersRow, ListInput as ScopeListInput,
    ListOutput as ScopeListOutput, OwnersInput, OwnersOutput, ScopeDetail, ScopeSummary,
    ShowInput as ScopeShowInput, ShowOutput as ScopeShowOutput,
};
use ft_ops::search::{GlobalSearchHit, GlobalSearchInput, GlobalSearchOutput, SearchKind};
use ft_ops::tickets::{
    BoardCard, BoardInput, BoardOutput, ClaimInput, CloseInput, CreateBugInput, CreateEpicInput,
    CreateSubtaskInput, CreateTaskInput, LinkInput, ListInput, ListedTicket, ShowInput,
    TicketKindFilter, TicketPriority, TicketRelationKind, TicketStatusFilter, UnclaimInput,
    UpdateInput,
};
use ft_ops::trust::{
    EvidenceKindInput, MergeInput, PromoteInput, ReasonInput, ReviewInput as TrustReviewInput,
    SupersedeInput,
};
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
#[allow(clippy::too_many_lines)]
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
    // Docs panel (firetrail-2mwp.8).
    DocView::export_all_to(dir).context("export DocView")?;
    DocFreshnessView::export_all_to(dir).context("export DocFreshnessView")?;
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
    // Cross-domain (unified) search op types.
    SearchKind::export_all_to(dir).context("export SearchKind")?;
    GlobalSearchInput::export_all_to(dir).context("export GlobalSearchInput")?;
    GlobalSearchHit::export_all_to(dir).context("export GlobalSearchHit")?;
    GlobalSearchOutput::export_all_to(dir).context("export GlobalSearchOutput")?;
    SalvageEntryAction::export_all_to(dir).context("export SalvageEntryAction")?;
    SalvageEntry::export_all_to(dir).context("export SalvageEntry")?;
    SalvageInput::export_all_to(dir).context("export SalvageInput")?;
    SalvageOutput::export_all_to(dir).context("export SalvageOutput")?;
    // Scope ops (W3-A).
    ScopeListInput::export_all_to(dir).context("export scope ListInput")?;
    ScopeListOutput::export_all_to(dir).context("export scope ListOutput")?;
    ScopeShowInput::export_all_to(dir).context("export scope ShowInput")?;
    ScopeShowOutput::export_all_to(dir).context("export scope ShowOutput")?;
    ScopeSummary::export_all_to(dir).context("export ScopeSummary")?;
    ScopeDetail::export_all_to(dir).context("export ScopeDetail")?;
    CodeOwnersRow::export_all_to(dir).context("export CodeOwnersRow")?;
    AliasesInput::export_all_to(dir).context("export AliasesInput")?;
    AliasesOutput::export_all_to(dir).context("export AliasesOutput")?;
    AliasEntry::export_all_to(dir).context("export AliasEntry")?;
    OwnersInput::export_all_to(dir).context("export OwnersInput")?;
    OwnersOutput::export_all_to(dir).context("export OwnersOutput")?;
    // Identity ops (W3-A).
    IdentityListInput::export_all_to(dir).context("export identity ListInput")?;
    IdentityListOutput::export_all_to(dir).context("export identity ListOutput")?;
    IdentityShowInput::export_all_to(dir).context("export identity ShowInput")?;
    IdentityShowOutput::export_all_to(dir).context("export identity ShowOutput")?;
    IdentityView::export_all_to(dir).context("export IdentityView")?;
    IdentityStatusFilter::export_all_to(dir).context("export IdentityStatusFilter")?;
    IdentityKindInput::export_all_to(dir).context("export IdentityKindInput")?;
    RegisterInput::export_all_to(dir).context("export RegisterInput")?;
    IdentityRegisterOutput::export_all_to(dir).context("export IdentityRegisterOutput")?;
    OffboardInput::export_all_to(dir).context("export OffboardInput")?;
    CapabilitiesInput::export_all_to(dir).context("export CapabilitiesInput")?;
    CapabilitiesOutput::export_all_to(dir).context("export CapabilitiesOutput")?;
    CapabilityRow::export_all_to(dir).context("export CapabilityRow")?;
    // Trust ops (W3-A).
    EvidenceKindInput::export_all_to(dir).context("export EvidenceKindInput")?;
    TrustReviewInput::export_all_to(dir).context("export trust ReviewInput")?;
    PromoteInput::export_all_to(dir).context("export PromoteInput")?;
    ReasonInput::export_all_to(dir).context("export ReasonInput")?;
    SupersedeInput::export_all_to(dir).context("export SupersedeInput")?;
    MergeInput::export_all_to(dir).context("export MergeInput")?;
    // Audit ops (W3-A).
    LintInput::export_all_to(dir).context("export LintInput")?;
    LintOutput::export_all_to(dir).context("export LintOutput")?;
    LintFinding::export_all_to(dir).context("export LintFinding")?;
    LintSeverity::export_all_to(dir).context("export LintSeverity")?;
    VerifyInput::export_all_to(dir).context("export VerifyInput")?;
    VerifyOutput::export_all_to(dir).context("export VerifyOutput")?;
    VerifyResult::export_all_to(dir).context("export VerifyResult")?;
    AuditReviewInput::export_all_to(dir).context("export audit ReviewInput")?;
    AuditReviewOutput::export_all_to(dir).context("export audit ReviewOutput")?;
    ReviewAcRow::export_all_to(dir).context("export ReviewAcRow")?;
    ReviewEvidenceRow::export_all_to(dir).context("export ReviewEvidenceRow")?;
    ReviewHistoryRow::export_all_to(dir).context("export ReviewHistoryRow")?;
    CriteriaAddInput::export_all_to(dir).context("export CriteriaAddInput")?;
    CriteriaListInput::export_all_to(dir).context("export CriteriaListInput")?;
    CriteriaListOutput::export_all_to(dir).context("export CriteriaListOutput")?;
    CriteriaListRow::export_all_to(dir).context("export CriteriaListRow")?;
    CriteriaToggleInput::export_all_to(dir).context("export CriteriaToggleInput")?;
    CriteriaEvidenceInput::export_all_to(dir).context("export CriteriaEvidenceInput")?;
    DiffInput::export_all_to(dir).context("export DiffInput")?;
    DiffOutput::export_all_to(dir).context("export DiffOutput")?;
    DiffRow::export_all_to(dir).context("export DiffRow")?;
    DiffChange::export_all_to(dir).context("export DiffChange")?;
    GraphInput::export_all_to(dir).context("export GraphInput")?;
    GraphOutput::export_all_to(dir).context("export GraphOutput")?;
    GraphNode::export_all_to(dir).context("export GraphNode")?;
    GraphEdge::export_all_to(dir).context("export GraphEdge")?;
    GraphDirectionInput::export_all_to(dir).context("export GraphDirectionInput")?;
    Ok(())
}

fn diff_dirs(committed: &Path, generated: &Path) -> Result<()> {
    use std::collections::BTreeSet;

    let committed_files = read_ts_files(committed)?;
    let generated_files = read_ts_files(generated)?;

    let mut missing: Vec<String> = Vec::new();
    let mut extra: Vec<String> = Vec::new();
    let mut changed: Vec<String> = Vec::new();

    let all_names: BTreeSet<&String> = committed_files
        .keys()
        .chain(generated_files.keys())
        .collect();

    for name in &all_names {
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
        eprintln!("content drift: {changed:?}");
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
        if !std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
        {
            // Keep the .gitkeep / READMEs ignored.
            continue;
        }
        let content = fs::read_to_string(&path)?;
        out.insert(name, content);
    }
    Ok(out)
}
