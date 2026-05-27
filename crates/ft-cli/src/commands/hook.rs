//! `firetrail _hook …` — internal hook entrypoints invoked by git.
//!
//! These commands are not user-facing; they are wired up by `firetrail init`
//! into the standard `post-checkout` and `post-merge` hooks. Their job at M2
//! is purely advisory: emit a warning to stderr when memory records would be
//! lost, and never block the underlying git operation. ADR-0018 specifies
//! richer behavior (interactive prompts, PR auto-open) — those are filed as
//! follow-ups.

use std::path::PathBuf;

use ft_git::{ChangeKind, Repo};
use ft_storage::{ChangeClass, RECORDS_DIR, classify_change};
use serde::Serialize;

use crate::cli::{GlobalOpts, HookOnCheckoutArgs, HookOnMergeArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const CMD_CHECKOUT: &str = "_hook on-checkout";
const CMD_MERGE: &str = "_hook on-merge";

/// JSON envelope for `_hook on-checkout` / `_hook on-merge`.
#[derive(Debug, Clone, Serialize)]
pub struct HookOutcome {
    /// Stable command name (`_hook on-checkout` / `_hook on-merge`).
    #[serde(skip)]
    pub command: &'static str,
    /// Short status label (`ok`, `warned`, `skipped`).
    pub status: String,
    /// Repo-relative paths of memory records that would benefit from salvage.
    pub flagged_paths: Vec<String>,
    /// Human-readable note for the operator (also echoed to stderr).
    pub note: String,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl HookOutcome {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        format!("{}: {}\n", self.command, self.note)
    }

    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("{}: {}", self.command, self.status)
    }
}

/// `firetrail _hook on-checkout <prev> <new> <branch_flag>`
pub fn on_checkout(
    args: &HookOnCheckoutArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    // File-level checkout (`branch_flag` == "0") is uninteresting — no branch
    // is being left behind.
    if args.branch_flag.trim() != "1" {
        return Ok(skipped(
            CMD_CHECKOUT,
            "file-level checkout — nothing to check",
        ));
    }

    // Hooks must never block on a missing workspace.
    let Ok(ws) = workspace::require_initialised(CMD_CHECKOUT, global.workspace.as_deref()) else {
        return Ok(skipped(
            CMD_CHECKOUT,
            "workspace not initialised — skipping",
        ));
    };

    // No-op when the SHAs are identical (e.g. `git checkout <branch>` of the
    // current branch). Avoids spurious warnings on routine no-op checkouts.
    if args.prev_ref == args.new_ref {
        return Ok(skipped(CMD_CHECKOUT, "no ref change"));
    }

    let repo = Repo::open(&ws.root).map_err(|e| CliError::internal(CMD_CHECKOUT, e))?;
    // Diff prev..new restricted to records. Anything that *disappeared* (or
    // changed) when leaving `prev` is potentially-lost memory.
    let entries = repo
        .diff(&args.new_ref, &args.prev_ref, Some(RECORDS_DIR))
        .map_err(|e| CliError::internal(CMD_CHECKOUT, format!("diff: {e}")))?;

    let flagged: Vec<PathBuf> = entries
        .into_iter()
        .filter(|e| matches!(e.change_kind, ChangeKind::Added | ChangeKind::Modified))
        .filter(|e| matches!(classify_change(&e.path), ChangeClass::Memory(_)))
        .map(|e| e.path)
        .collect();

    if flagged.is_empty() {
        return Ok(ok(
            CMD_CHECKOUT,
            "no unsalvaged memory records on previous branch",
        ));
    }

    let paths: Vec<String> = flagged.iter().map(|p| p.display().to_string()).collect();
    let note = format!(
        "{} memory record(s) on the branch you just left are not on the new branch. \
         Consider running `firetrail memory salvage` before deleting it.",
        paths.len()
    );
    // Stderr emission is the M2 "prompt" — non-blocking by design.
    eprintln!("firetrail: {note}");
    for p in &paths {
        eprintln!("  - {p}");
    }

    Ok(CommandOutcome::Hook(HookOutcome {
        command: CMD_CHECKOUT,
        status: "warned".into(),
        flagged_paths: paths,
        note,
        warnings: Vec::new(),
    }))
}

/// `firetrail _hook on-merge <squash_flag>`
///
/// Returns a `Result` for dispatcher symmetry with sibling commands, even
/// though M2 has no failure path. Richer salvage triggers (interactive
/// prompts, network sync) will add real error variants.
#[allow(clippy::unnecessary_wraps)]
pub fn on_merge(_args: &HookOnMergeArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let Ok(_ws) = workspace::require_initialised(CMD_MERGE, global.workspace.as_deref()) else {
        return Ok(skipped(CMD_MERGE, "workspace not initialised — skipping"));
    };
    // M2 behaviour: if the working tree is on `main` (or a branch that
    // contains the merge), every record that landed via the merge is now
    // durable — there's nothing to warn about. We surface a quiet status so
    // the hook's JSON envelope still parses cleanly for any tooling consuming
    // it.
    Ok(ok(CMD_MERGE, "merge completed — no salvage action needed"))
}

fn ok(command: &'static str, note: &str) -> CommandOutcome {
    CommandOutcome::Hook(HookOutcome {
        command,
        status: "ok".into(),
        flagged_paths: Vec::new(),
        note: note.to_string(),
        warnings: Vec::new(),
    })
}

fn skipped(command: &'static str, note: &str) -> CommandOutcome {
    CommandOutcome::Hook(HookOutcome {
        command,
        status: "skipped".into(),
        flagged_paths: Vec::new(),
        note: note.to_string(),
        warnings: Vec::new(),
    })
}
