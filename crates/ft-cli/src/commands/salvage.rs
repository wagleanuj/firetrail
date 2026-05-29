//! `firetrail memory salvage` — rescue memory records from an
//! abandoned / about-to-be-deleted branch into a dedicated salvage branch.
//!
//! See ADR-0018 for the workflow specification. At M2, salvage operates
//! locally only — the memory-only PR step is left as a follow-up:
//!
//! 1. Walk records on `HEAD` that are not present on `--base` (default `main`).
//! 2. Classify each path with [`ft_storage::classify_change`].
//! 3. Decide per record: memory-kind defaults to *salvage*, structural-kind
//!    defaults to *skip*. In interactive mode the operator may override; in
//!    `--auto` / `--non-interactive` / `--dry-run` mode the defaults stand.
//! 4. If anything was selected and we're not dry-running, copy the chosen
//!    record blobs onto a new `salvage/<base>-from-<source>-<ts>` branch
//!    cut from `--base`, commit, and report the branch name.
//!
//! The output envelope is stable (`SalvageOutcome` below) and the basis for
//! both the test assertions and downstream automation (eg. opening the
//! memory-only PR per ADR-0009).

use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use ft_core::RecordKind;
use ft_git::Repo;
use ft_storage::{ChangeClass, classify_change};
use serde::Serialize;

use crate::cli::{GlobalOpts, MemorySalvageArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const COMMAND: &str = "memory salvage";

/// Per-record decision returned to the operator.
#[derive(Debug, Clone, Serialize)]
pub struct SalvageEntry {
    /// Record id derived from the on-disk filename (`<ID>.json` → uppercased).
    pub id: String,
    /// Record kind (`finding`, `task`, …).
    pub kind: String,
    /// `salvaged` or `skipped`.
    pub action: String,
    /// Human-readable reason for the chosen action.
    pub reason: String,
    /// Repo-relative path of the record's blob on the source branch.
    pub path: String,
}

/// JSON envelope for `memory salvage`.
#[derive(Debug, Clone, Serialize)]
pub struct SalvageOutcome {
    /// Base ref the diff was computed against (defaulted to `main`).
    pub base: String,
    /// Source branch that records were salvaged *from*. `None` if the source
    /// is detached HEAD (the SHA is reported via `source_ref` instead).
    pub source_branch: Option<String>,
    /// Resolved source ref (branch name or SHA when detached).
    pub source_ref: String,
    /// Per-record decisions.
    pub entries: Vec<SalvageEntry>,
    /// Name of the new salvage branch, when records were salvaged for real.
    /// `None` when nothing was salvaged or `--dry-run` was set.
    pub salvage_branch: Option<String>,
    /// `true` iff the run was a planning pass (no mutation).
    pub dry_run: bool,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl SalvageOutcome {
    /// Number of records actually salvaged.
    #[must_use]
    pub fn salvaged_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.action == "salvaged")
            .count()
    }

    /// Number of records skipped.
    #[must_use]
    pub fn skipped_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.action == "skipped")
            .count()
    }

    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# memory salvage\n\nSource: `{}` · Base: `{}`{}\n",
            self.source_ref,
            self.base,
            if self.dry_run { " · dry-run" } else { "" }
        );
        if self.entries.is_empty() {
            s.push_str("No memory records to salvage. ✅\n");
            return s;
        }
        s.push_str("| Action | Kind | Id | Reason |\n|--------|------|----|--------|\n");
        for e in &self.entries {
            let _ = writeln!(
                s,
                "| {} | {} | `{}` | {} |",
                e.action, e.kind, e.id, e.reason
            );
        }
        if let Some(b) = &self.salvage_branch {
            let _ = writeln!(s, "\nSalvage branch: `{b}`");
        }
        s
    }

    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!(
            "memory salvage: {} salvaged, {} skipped{}",
            self.salvaged_count(),
            self.skipped_count(),
            self.salvage_branch
                .as_deref()
                .map(|b| format!(" → {b}"))
                .unwrap_or_default(),
        )
    }
}

/// Entry point for `firetrail memory salvage`.
#[allow(clippy::too_many_lines)]
pub fn run(args: &MemorySalvageArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(COMMAND, global.workspace.as_deref())?;
    let mut warnings = Vec::new();

    let repo = Repo::open(&ws.root).map_err(|e| CliError::internal(COMMAND, e))?;

    // Resolve source ref. If --branch was given, use it; otherwise current branch
    // (or HEAD if detached).
    let (source_branch, source_ref) = match &args.branch {
        Some(b) => {
            if !repo
                .branch_exists(b)
                .map_err(|e| CliError::internal(COMMAND, e))?
            {
                return Err(CliError::user(
                    COMMAND,
                    format!("source branch `{b}` does not exist"),
                ));
            }
            (Some(b.clone()), b.clone())
        }
        None => match repo
            .current_branch()
            .map_err(|e| CliError::internal(COMMAND, e))?
        {
            Some(b) => (Some(b.clone()), b),
            None => (None, "HEAD".to_string()),
        },
    };

    // If the source matches the base, there is nothing to salvage from a
    // diverged branch — surface a clear no-op envelope.
    if source_branch.as_deref() == Some(args.base.as_str()) {
        warnings.push(format!(
            "source `{0}` is the same as base `{0}`; nothing can be salvaged",
            args.base
        ));
        return Ok(CommandOutcome::MemorySalvage(SalvageOutcome {
            base: args.base.clone(),
            source_branch,
            source_ref,
            entries: Vec::new(),
            salvage_branch: None,
            dry_run: args.dry_run,
            warnings,
        }));
    }

    if !repo
        .branch_exists(&args.base)
        .map_err(|e| CliError::internal(COMMAND, e))?
    {
        return Err(CliError::user(
            COMMAND,
            format!("base branch `{}` does not exist", args.base),
        ));
    }

    // List record blobs at source and at base. The candidate set is
    // (source ∖ base) plus any path whose blob differs from base — i.e.
    // anything that would be lost or downgraded by abandoning this branch.
    //
    // We avoid `Repo::diff` here because at M2 ft-git surfaces tree-level
    // change entries (gix's default), which produces noisy directory-only
    // events for newly-created kind subdirectories. Two `list_files_at_ref`
    // calls give us per-blob granularity without extending ft-git's surface.
    let source_paths = repo
        .list_files_at_ref(&source_ref, ".firetrail/records/**/*.json")
        .map_err(|e| CliError::internal(COMMAND, format!("list source records: {e}")))?;
    let base_paths: HashSet<PathBuf> = repo
        .list_files_at_ref(&args.base, ".firetrail/records/**/*.json")
        .map_err(|e| CliError::internal(COMMAND, format!("list base records: {e}")))?
        .into_iter()
        .collect();

    let interactive = is_interactive(args);
    let mut candidates: Vec<SalvageEntry> = Vec::new();
    for path in source_paths {
        // Path present on both sides — only include when the blob differs
        // (i.e. this branch advances or rewrites it). Pure path-presence
        // without content change is not interesting.
        if base_paths.contains(&path) {
            let src_blob = repo.read_file_at_ref(&source_ref, &path).map_err(|e| {
                CliError::internal(COMMAND, format!("read source {}: {e}", path.display()))
            })?;
            let base_blob = repo.read_file_at_ref(&args.base, &path).map_err(|e| {
                CliError::internal(COMMAND, format!("read base {}: {e}", path.display()))
            })?;
            if src_blob == base_blob {
                continue;
            }
        }
        let class = classify_change(&path);
        let kind = match class {
            ChangeClass::Memory(k) | ChangeClass::Structural(k) => k,
            // Config / Other: defensive — `list_files_at_ref` already
            // restricted to `.firetrail/records/**/*.json`, so this is
            // unreachable in practice but cheap to handle.
            ChangeClass::Config | ChangeClass::Other => continue,
        };
        let id = id_from_path(&path).unwrap_or_else(|| path.display().to_string());
        let (action, reason) = decide(kind, &id, interactive);
        candidates.push(SalvageEntry {
            id,
            kind: kind_label(kind).to_string(),
            action: action.to_string(),
            reason,
            path: path.display().to_string(),
        });
    }

    // If we accumulated anything to salvage, perform the salvage.
    let to_salvage: Vec<&SalvageEntry> = candidates
        .iter()
        .filter(|e| e.action == "salvaged")
        .collect();
    let salvage_branch = if to_salvage.is_empty() || args.dry_run {
        None
    } else {
        Some(perform_salvage(
            &repo,
            &ws.root,
            &args.base,
            &source_ref,
            source_branch.as_deref(),
            &to_salvage,
        )?)
    };

    Ok(CommandOutcome::MemorySalvage(SalvageOutcome {
        base: args.base.clone(),
        source_branch,
        source_ref,
        entries: candidates,
        salvage_branch,
        dry_run: args.dry_run,
        warnings,
    }))
}

/// Resolve whether the operator is in interactive mode. `--auto` /
/// `--non-interactive` / `--dry-run` and a non-TTY stdin all force
/// non-interactive (in which case [`decide`] uses kind-based defaults).
fn is_interactive(args: &MemorySalvageArgs) -> bool {
    if args.auto || args.non_interactive || args.dry_run {
        return false;
    }
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Default action for `kind`. Memory kinds salvage by default; structural
/// kinds skip by default (ADR-0018).
fn default_action(kind: RecordKind) -> (&'static str, String) {
    if ft_storage::is_memory_kind(kind) {
        (
            "salvaged",
            format!("memory kind `{}` salvages by default", kind_label(kind)),
        )
    } else {
        (
            "skipped",
            format!(
                "workflow kind `{}` does not salvage by default (ADR-0018)",
                kind_label(kind)
            ),
        )
    }
}

/// Decide per record: kind-based defaults when non-interactive; prompt the
/// operator when interactive. Returns `(action, reason)`.
fn decide(kind: RecordKind, id: &str, interactive: bool) -> (&'static str, String) {
    use crate::prompt::{PromptChoice, ask};

    let (default_action_str, default_reason) = default_action(kind);
    if !interactive {
        return (default_action_str, default_reason);
    }
    let default_choice = if default_action_str == "salvaged" {
        PromptChoice::Yes
    } else {
        PromptChoice::No
    };
    let default_label = if default_choice == PromptChoice::Yes {
        "Y/n"
    } else {
        "y/N"
    };
    let q = format!("salvage `{id}` ({})? [{default_label}]", kind_label(kind));
    let choice = ask(&q, default_choice).unwrap_or(default_choice);
    match choice {
        PromptChoice::Yes => ("salvaged", format!("operator selected salvage for `{id}`")),
        PromptChoice::No | PromptChoice::Quit => {
            ("skipped", format!("operator selected skip for `{id}`"))
        }
    }
}

/// Cut a salvage branch from `base`, copy the selected record blobs into the
/// working tree, commit, and switch back to the original source ref.
fn perform_salvage(
    repo: &Repo,
    repo_root: &Path,
    base: &str,
    source_ref: &str,
    source_branch: Option<&str>,
    entries: &[&SalvageEntry],
) -> Result<String, CliError> {
    // Snapshot the blobs we want to salvage *before* switching branches.
    // We read from the source ref via gix to avoid any working-tree fights.
    let mut blobs: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(entries.len());
    for e in entries {
        let path = PathBuf::from(&e.path);
        let bytes = repo.read_file_at_ref(source_ref, &path).map_err(|err| {
            CliError::internal(COMMAND, format!("read {} at source: {err}", path.display()))
        })?;
        blobs.push((path, bytes));
    }

    // Name the branch deterministically per ADR-0018 sketch.
    let ts = Utc::now().format("%Y%m%d%H%M%S");
    let source_label = source_branch.unwrap_or("detached");
    let branch_name = format!("salvage/{base}-from-{source_label}-{ts}");

    // Refuse if the workspace is dirty — salvage uses a real `git checkout`
    // and we don't want to silently swallow the operator's working changes.
    if !repo
        .is_clean()
        .map_err(|e| CliError::internal(COMMAND, e))?
    {
        return Err(CliError::user(
            COMMAND,
            "working tree has uncommitted changes; commit or stash before running salvage",
        ));
    }

    // Cut the branch from base and check it out. We shell out so we get
    // porcelain semantics, mirroring how the rest of ft-git handles writes.
    git(
        repo_root,
        &["checkout", "--quiet", "-b", &branch_name, base],
    )?;

    // Write each blob into the working tree and stage it.
    for (rel, bytes) in &blobs {
        let abs = repo_root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CliError::internal(COMMAND, format!("mkdir {}: {e}", parent.display()))
            })?;
        }
        std::fs::write(&abs, bytes)
            .map_err(|e| CliError::internal(COMMAND, format!("write {}: {e}", abs.display())))?;
        git(repo_root, &["add", "--", rel.to_string_lossy().as_ref()])?;
    }

    let msg = format!(
        "salvage: {} memory record(s) from {source_label}",
        blobs.len()
    );
    git(repo_root, &["commit", "--quiet", "-m", &msg])?;

    // Return to the source ref so the operator's HEAD doesn't shift under
    // their feet. If the source was detached, leave the operator on the
    // salvage branch — they explicitly opted in to a branch-y workflow.
    if let Some(name) = source_branch {
        git(repo_root, &["checkout", "--quiet", name])?;
    }

    Ok(branch_name)
}

/// Run a `git` subcommand under `repo_root`, surfacing failures as internal
/// errors. Mirrors `ft_git::Repo::run_git` but stays in ft-cli so we don't
/// have to extend ft-git's public surface.
fn git(repo_root: &Path, args: &[&str]) -> Result<(), CliError> {
    let out = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| CliError::internal(COMMAND, format!("spawn git {args:?}: {e}")))?;
    if !out.status.success() {
        return Err(CliError::internal(
            COMMAND,
            format!(
                "git {args:?} failed (exit {}): {}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(())
}

/// Extract the canonical record id from a `.firetrail/records/<kind>/<lower-id>.json` path.
fn id_from_path(p: &Path) -> Option<String> {
    let stem = p.file_stem()?.to_str()?;
    let (prefix, _hex) = stem.split_once('-')?;
    Some(format!(
        "{}-{}",
        prefix.to_uppercase(),
        &stem[prefix.len() + 1..]
    ))
}

/// Lowercase short name for a kind, as used in JSON / markdown output.
fn kind_label(k: RecordKind) -> &'static str {
    match k {
        RecordKind::Task => "task",
        RecordKind::Epic => "epic",
        RecordKind::Subtask => "subtask",
        RecordKind::Bug => "bug",
        RecordKind::Incident => "incident",
        RecordKind::Finding => "finding",
        RecordKind::Runbook => "runbook",
        RecordKind::Decision => "decision",
        RecordKind::Gotcha => "gotcha",
        RecordKind::Memory => "memory",
        RecordKind::Doc => "doc",
    }
}
