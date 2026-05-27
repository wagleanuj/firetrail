//! `firetrail merge-driver-install` — install the Firetrail JSON merge driver
//! into the current git repository.
//!
//! Writes (idempotently):
//!
//! - `.gitattributes` entry: `.firetrail/records/**/*.json merge=firetrail`
//! - `.git/config` driver entry:
//!   ```ini
//!   [merge "firetrail"]
//!       name = Firetrail record three-way merge
//!       driver = firetrail-merge-driver %O %A %B
//!   ```
//!
//! Both steps are idempotent: re-running the command leaves the repo in the
//! same state.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::cli::{GlobalOpts, MergeDriverInstallArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "merge-driver-install";

const GITATTRIBUTES_LINE: &str = ".firetrail/records/**/*.json merge=firetrail";

const DEFAULT_DRIVER_BIN: &str = "firetrail-merge-driver";

/// `firetrail merge-driver-install`
pub fn install(
    args: &MergeDriverInstallArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let root = ctx.ws.root.clone();

    let bin = args
        .binary
        .clone()
        .unwrap_or_else(|| DEFAULT_DRIVER_BIN.to_string());

    // Ensure .gitattributes contains the merge attribute.
    let gitattributes = root.join(".gitattributes");
    let added_gitattributes = ensure_line(&gitattributes, GITATTRIBUTES_LINE)?;

    // Ensure .git/config has the merge.firetrail section.
    let git_config = root.join(".git").join("config");
    let driver_line = format!("{bin} %O %A %B");
    let added_config = ensure_git_merge_driver(&git_config, &driver_line)?;

    Ok(CommandOutcome::MergeDriverInstall(
        MergeDriverInstallOutcome {
            gitattributes_path: gitattributes.display().to_string(),
            git_config_path: git_config.display().to_string(),
            driver_binary: bin,
            added_gitattributes,
            added_git_config: added_config,
            warnings,
        },
    ))
}

fn ensure_line(path: &Path, line: &str) -> Result<bool, CliError> {
    let mut existing = String::new();
    if path.exists() {
        existing = fs::read_to_string(path)
            .map_err(|e| CliError::internal(COMMAND, format!("read {}: {e}", path.display())))?;
        if existing.lines().any(|l| l.trim() == line) {
            return Ok(false);
        }
    }
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(line);
    existing.push('\n');
    fs::write(path, existing)
        .map_err(|e| CliError::internal(COMMAND, format!("write {}: {e}", path.display())))?;
    Ok(true)
}

/// Idempotent append/update of the `[merge "firetrail"]` section in
/// `.git/config`. Returns `true` when the file was mutated, `false` when it
/// was already in the desired state.
fn ensure_git_merge_driver(path: &Path, driver_line: &str) -> Result<bool, CliError> {
    let existing = if path.exists() {
        fs::read_to_string(path)
            .map_err(|e| CliError::internal(COMMAND, format!("read {}: {e}", path.display())))?
    } else {
        String::new()
    };

    let want_section = "[merge \"firetrail\"]";
    let want_name = "name = Firetrail record three-way merge";
    let want_driver = format!("driver = {driver_line}");

    // Cheap idempotency: if the existing file already contains the section
    // header *and* the exact driver line, leave it alone.
    if existing.contains(want_section) && existing.contains(&want_driver) {
        return Ok(false);
    }

    // Strip any prior `[merge "firetrail"]` block so we can rewrite cleanly.
    let stripped = strip_merge_firetrail_section(&existing);

    let mut next = stripped;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(want_section);
    next.push('\n');
    next.push('\t');
    next.push_str(want_name);
    next.push('\n');
    next.push('\t');
    next.push_str(&want_driver);
    next.push('\n');

    fs::write(path, next)
        .map_err(|e| CliError::internal(COMMAND, format!("write {}: {e}", path.display())))?;
    Ok(true)
}

fn strip_merge_firetrail_section(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_section = trimmed.starts_with("[merge \"firetrail\"]");
            if in_section {
                continue;
            }
        }
        if in_section {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Outcome of `merge-driver-install`.
#[derive(Debug, Clone, Serialize)]
pub struct MergeDriverInstallOutcome {
    /// Path of the `.gitattributes` file managed.
    pub gitattributes_path: String,
    /// Path of the `.git/config` file managed.
    pub git_config_path: String,
    /// Driver binary recorded in the config (defaults to
    /// `firetrail-merge-driver`).
    pub driver_binary: String,
    /// Whether the `.gitattributes` line was added on this run.
    pub added_gitattributes: bool,
    /// Whether the `.git/config` block was rewritten on this run.
    pub added_git_config: bool,
    /// Non-fatal CLI warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl MergeDriverInstallOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        format!(
            "# merge-driver-install\n\n- gitattributes: `{}` ({})\n- git config: `{}` ({})\n- driver: `{}`\n",
            self.gitattributes_path,
            if self.added_gitattributes {
                "added"
            } else {
                "already present"
            },
            self.git_config_path,
            if self.added_git_config {
                "added"
            } else {
                "already present"
            },
            self.driver_binary,
        )
    }

    /// One-line summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "merge-driver-install: gitattributes={} git_config={}",
            if self.added_gitattributes {
                "added"
            } else {
                "ok"
            },
            if self.added_git_config { "added" } else { "ok" },
        )
    }
}

/// Test seam: expose the `.gitattributes` line so integration tests can
/// assert on it without re-importing the constant.
#[must_use]
#[allow(dead_code)]
pub fn gitattributes_line() -> &'static str {
    GITATTRIBUTES_LINE
}

/// Test seam: build the expected driver command line.
#[must_use]
#[allow(dead_code)]
pub fn expected_driver_line(bin: &str) -> String {
    format!("{bin} %O %A %B")
}
