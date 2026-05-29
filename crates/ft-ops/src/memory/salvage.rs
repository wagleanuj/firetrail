//! ADR-0018 memory salvage — rescue records from an abandoned branch
//! into a dedicated salvage branch.
//!
//! Wave 2-A op shape: the CLI's interactive `accept/reject/quit` prompt is
//! replaced by an explicit selection mechanism on [`SalvageInput`]:
//!
//! - `selected = None` (default): apply kind-based defaults (memory kinds
//!   salvage, structural kinds skip). Equivalent to CLI `--auto`.
//! - `selected = Some(ids)`: explicit accept-list; record ids present in
//!   the list are salvaged, all others are skipped. The GUI workflow is
//!   "call salvage with `dry_run = true` to list candidates → operator
//!   ticks the ones to keep → call again with `dry_run = false` and the
//!   chosen ids".
//!
//! Every per-record decision emits [`crate::Event::MemorySalvaged`] with
//! the resolved [`crate::events::SalvageDecision`] (accepted / rejected).
//!
//! Git access is via shell-out (`git`), mirroring the CLI implementation:
//! ft-git's diff surface is tree-level and would produce noisy
//! directory-only events for new kind subdirectories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use ft_core::RecordKind;
use ft_git::Repo;
use ft_storage::{ChangeClass, classify_change};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus, SalvageDecision};
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Per-record decision in [`SalvageOutput`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SalvageEntryAction {
    /// Record was (or would be, in dry-run) copied onto the salvage branch.
    Salvaged,
    /// Record was deliberately skipped.
    Skipped,
}

impl SalvageEntryAction {
    fn as_decision(self) -> SalvageDecision {
        match self {
            Self::Salvaged => SalvageDecision::Accepted,
            Self::Skipped => SalvageDecision::Rejected,
        }
    }
}

/// One candidate record's outcome.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvageEntry {
    /// Canonical record id (uppercased prefix).
    pub id: String,
    /// Memory kind (`finding`, `task`, …).
    pub kind: String,
    /// Selected action.
    pub action: SalvageEntryAction,
    /// Human-readable reason for the action.
    pub reason: String,
    /// Repo-relative path of the record blob on the source branch.
    pub path: String,
}

/// Input for [`salvage`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvageInput {
    /// Base branch to compare against. Defaults to `"main"`.
    #[serde(default = "default_base")]
    pub base: String,
    /// Salvage source branch. `None` means the current branch (or HEAD
    /// if detached).
    #[serde(default)]
    pub branch: Option<String>,
    /// Plan-only: do not mutate the repo. Use this to enumerate candidates
    /// before submitting an explicit selection.
    #[serde(default)]
    pub dry_run: bool,
    /// Explicit accept-list of record ids. `None` falls back to kind-based
    /// defaults (memory salvages, structural skips). When `Some`, every
    /// candidate whose id is not in the list is force-skipped.
    #[serde(default)]
    pub selected: Option<Vec<String>>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn default_base() -> String {
    "main".to_string()
}

/// Output of [`salvage`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvageOutput {
    /// Base branch the diff was computed against.
    pub base: String,
    /// Source branch (when not detached).
    pub source_branch: Option<String>,
    /// Resolved source ref (branch name or SHA when detached).
    pub source_ref: String,
    /// Per-record decisions.
    pub entries: Vec<SalvageEntry>,
    /// New salvage branch name. `None` when nothing was salvaged or
    /// `dry_run = true`.
    pub salvage_branch: Option<String>,
    /// Mirrors `input.dry_run`.
    pub dry_run: bool,
    /// Non-fatal warnings.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// `memory salvage` op.
#[allow(clippy::too_many_lines)]
pub fn salvage(
    ws: &Workspace,
    _identity: &Identity,
    input: SalvageInput,
    events: &EventBus,
) -> Result<SalvageOutput, OpsError> {
    let mut warnings: Vec<String> = Vec::new();
    let repo =
        Repo::open(&ws.root).map_err(|e| OpsError::Internal(anyhow::anyhow!("open repo: {e}")))?;

    // Resolve source ref.
    let (source_branch, source_ref) = match &input.branch {
        Some(b) => {
            if !repo
                .branch_exists(b)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("branch_exists: {e}")))?
            {
                return Err(OpsError::validation(
                    "branch",
                    format!("source branch `{b}` does not exist"),
                ));
            }
            (Some(b.clone()), b.clone())
        }
        None => match repo
            .current_branch()
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("current_branch: {e}")))?
        {
            Some(b) => (Some(b.clone()), b),
            None => (None, "HEAD".to_string()),
        },
    };

    if source_branch.as_deref() == Some(input.base.as_str()) {
        warnings.push(format!(
            "source `{0}` is the same as base `{0}`; nothing can be salvaged",
            input.base
        ));
        return Ok(SalvageOutput {
            base: input.base,
            source_branch,
            source_ref,
            entries: Vec::new(),
            salvage_branch: None,
            dry_run: input.dry_run,
            warnings,
        });
    }

    if !repo
        .branch_exists(&input.base)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("branch_exists base: {e}")))?
    {
        return Err(OpsError::validation(
            "base",
            format!("base branch `{}` does not exist", input.base),
        ));
    }

    // List record blobs at source and at base.
    let source_paths = repo
        .list_files_at_ref(&source_ref, ".firetrail/records/**/*.json")
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list source records: {e}")))?;
    let base_paths: HashSet<PathBuf> = repo
        .list_files_at_ref(&input.base, ".firetrail/records/**/*.json")
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list base records: {e}")))?
        .into_iter()
        .collect();

    let selected_lookup: Option<HashSet<String>> = input
        .selected
        .as_ref()
        .map(|ids| ids.iter().map(|s| s.to_ascii_uppercase()).collect());

    let mut candidates: Vec<SalvageEntry> = Vec::new();
    for path in source_paths {
        if base_paths.contains(&path) {
            let src_blob = repo.read_file_at_ref(&source_ref, &path).map_err(|e| {
                OpsError::Internal(anyhow::anyhow!("read source {}: {e}", path.display()))
            })?;
            let base_blob = repo.read_file_at_ref(&input.base, &path).map_err(|e| {
                OpsError::Internal(anyhow::anyhow!("read base {}: {e}", path.display()))
            })?;
            if src_blob == base_blob {
                continue;
            }
        }
        let class = classify_change(&path);
        let kind = match class {
            ChangeClass::Memory(k) | ChangeClass::Structural(k) => k,
            ChangeClass::Config | ChangeClass::Other => continue,
        };
        let id = id_from_path(&path).unwrap_or_else(|| path.display().to_string());
        let (action, reason) = decide(kind, &id, selected_lookup.as_ref());
        candidates.push(SalvageEntry {
            id,
            kind: kind_label(kind).to_string(),
            action,
            reason,
            path: path.display().to_string(),
        });
    }

    let to_salvage: Vec<&SalvageEntry> = candidates
        .iter()
        .filter(|e| e.action == SalvageEntryAction::Salvaged)
        .collect();

    let salvage_branch = if to_salvage.is_empty() || input.dry_run {
        None
    } else {
        Some(perform_salvage(
            &repo,
            &ws.root,
            &input.base,
            &source_ref,
            source_branch.as_deref(),
            &to_salvage,
        )?)
    };

    // Emit MemorySalvaged for each candidate. Only fire events for non-
    // dry-run invocations so planning passes are silent on the bus.
    if !input.dry_run {
        for entry in &candidates {
            let event = Event::MemorySalvaged {
                id: entry.id.clone(),
                decision: entry.action.as_decision(),
            };
            if let Some(rid) = input.request_id.as_deref() {
                events.emit_with_request(rid.to_string(), event);
            } else {
                events.emit(event);
            }
        }
    }

    Ok(SalvageOutput {
        base: input.base,
        source_branch,
        source_ref,
        entries: candidates,
        salvage_branch,
        dry_run: input.dry_run,
        warnings,
    })
}

/// Decide per record. When `selected` is `Some`, ids in the set are accepted
/// and all others rejected. When `None`, fall back to kind-based defaults
/// (ADR-0018: memory kinds salvage, structural kinds skip).
fn decide(
    kind: RecordKind,
    id: &str,
    selected: Option<&HashSet<String>>,
) -> (SalvageEntryAction, String) {
    if let Some(set) = selected {
        if set.contains(&id.to_ascii_uppercase()) {
            return (
                SalvageEntryAction::Salvaged,
                format!("operator selected salvage for `{id}`"),
            );
        }
        return (
            SalvageEntryAction::Skipped,
            "not in operator's accept-list".to_string(),
        );
    }
    if ft_storage::is_memory_kind(kind) {
        (
            SalvageEntryAction::Salvaged,
            format!("memory kind `{}` salvages by default", kind_label(kind)),
        )
    } else {
        (
            SalvageEntryAction::Skipped,
            format!(
                "workflow kind `{}` does not salvage by default (ADR-0018)",
                kind_label(kind)
            ),
        )
    }
}

fn perform_salvage(
    repo: &Repo,
    repo_root: &Path,
    base: &str,
    source_ref: &str,
    source_branch: Option<&str>,
    entries: &[&SalvageEntry],
) -> Result<String, OpsError> {
    let mut blobs: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(entries.len());
    for e in entries {
        let path = PathBuf::from(&e.path);
        let bytes = repo.read_file_at_ref(source_ref, &path).map_err(|err| {
            OpsError::Internal(anyhow::anyhow!("read {} at source: {err}", path.display()))
        })?;
        blobs.push((path, bytes));
    }

    let ts = Utc::now().format("%Y%m%d%H%M%S");
    let source_label = source_branch.unwrap_or("detached");
    let branch_name = format!("salvage/{base}-from-{source_label}-{ts}");

    if !repo
        .is_clean()
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("is_clean: {e}")))?
    {
        return Err(OpsError::Conflict {
            reason: "working tree has uncommitted changes; commit or stash before running salvage"
                .to_string(),
        });
    }

    git(
        repo_root,
        &["checkout", "--quiet", "-b", &branch_name, base],
    )?;

    for (rel, bytes) in &blobs {
        let abs = repo_root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                OpsError::Internal(anyhow::anyhow!("mkdir {}: {e}", parent.display()))
            })?;
        }
        std::fs::write(&abs, bytes)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("write {}: {e}", abs.display())))?;
        git(repo_root, &["add", "--", rel.to_string_lossy().as_ref()])?;
    }

    let msg = format!(
        "salvage: {} memory record(s) from {source_label}",
        blobs.len()
    );
    git(repo_root, &["commit", "--quiet", "-m", &msg])?;

    if let Some(name) = source_branch {
        git(repo_root, &["checkout", "--quiet", name])?;
    }

    Ok(branch_name)
}

fn git(repo_root: &Path, args: &[&str]) -> Result<(), OpsError> {
    let out = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("spawn git {args:?}: {e}")))?;
    if !out.status.success() {
        return Err(OpsError::Internal(anyhow::anyhow!(
            "git {args:?} failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

fn id_from_path(p: &Path) -> Option<String> {
    let stem = p.file_stem()?.to_str()?;
    let (prefix, _hex) = stem.split_once('-')?;
    Some(format!(
        "{}-{}",
        prefix.to_uppercase(),
        &stem[prefix.len() + 1..]
    ))
}

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
