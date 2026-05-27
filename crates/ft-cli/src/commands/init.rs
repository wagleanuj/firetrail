//! `firetrail init` — bootstrap a workspace.
//!
//! Behaviour mirrors `docs/components/ft-cli.md`:
//!
//! 1. Verify cwd is in a git repo.
//! 2. Create `.firetrail/` and the per-kind `records/<type>/` directories.
//! 3. Write a default `config.yml` (or update an existing one in place).
//! 4. Stub `identity.yml` if absent.
//! 5. Initialise the `SQLite` index.
//! 6. Install git hooks (pre-commit, post-checkout, post-merge) via ft-git.
//! 7. Append `.firetrail/index.db` and `.firetrail/cache/` to `.gitignore`.
//! 8. Optionally write `AGENTS.md` and `.claude/skills/firetrail/SKILL.md`.
//!
//! The command is idempotent — re-running on an initialised workspace
//! refreshes hooks and ensures defaults are present without clobbering user
//! customisations.

use std::path::{Path, PathBuf};

use ft_git::{HookName, Repo};
use ft_index::Index;
use ft_storage::EmbeddedStorage;
use serde::Serialize;

use crate::cli::{GlobalOpts, InitArgs, StorageModeArg};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const COMMAND: &str = "init";

/// Per-step outcome of `firetrail init`.
#[derive(Debug, Clone, Serialize)]
pub struct InitReport {
    /// Repo root that was initialised.
    pub root: PathBuf,
    /// Whether this run was on a brand-new workspace.
    pub fresh: bool,
    /// Files / directories created or updated.
    pub created: Vec<String>,
    /// Files that already existed and were preserved.
    pub preserved: Vec<String>,
    /// Git hooks installed.
    pub hooks_installed: Vec<String>,
    /// Non-fatal warnings raised during init.
    pub warnings: Vec<String>,
}

impl InitReport {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# firetrail init\n\nWorkspace: `{}`\nFresh install: {}\n",
            self.root.display(),
            self.fresh
        );
        for (heading, items) in [
            ("## Created", &self.created),
            ("## Preserved", &self.preserved),
            ("## Hooks", &self.hooks_installed),
            ("## Warnings", &self.warnings),
        ] {
            if items.is_empty() {
                continue;
            }
            let _ = writeln!(s, "{heading}");
            for item in items {
                let _ = writeln!(s, "- {item}");
            }
            s.push('\n');
        }
        s.push_str("Done. Run `firetrail doctor` to verify.\n");
        s
    }

    /// One-line summary used in `--quiet` mode.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        if self.fresh {
            format!("initialised {}", self.root.display())
        } else {
            format!("refreshed {}", self.root.display())
        }
    }
}

/// Entry point.
pub fn run(args: &InitArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::locate(COMMAND, global.workspace.as_deref())?;
    let fresh = !ws.firetrail_dir().exists();

    let mut report = InitReport {
        root: ws.root.clone(),
        fresh,
        created: Vec::new(),
        preserved: Vec::new(),
        hooks_installed: Vec::new(),
        warnings: Vec::new(),
    };

    if matches!(args.storage_mode, StorageModeArg::External) {
        report
            .warnings
            .push("external storage mode is not yet available; falling back to embedded".into());
    }

    // 1. Records layout.
    EmbeddedStorage::init(&ws.root).map_err(|e| CliError::internal(COMMAND, e))?;
    track(&mut report, &ws.firetrail_dir(), ".firetrail/", fresh);
    track(
        &mut report,
        &ws.firetrail_dir().join("records"),
        ".firetrail/records/",
        fresh,
    );

    // 2. config.yml — default if missing, otherwise preserve.
    let config_path = ws.config_path();
    if config_path.exists() {
        report.preserved.push(".firetrail/config.yml".into());
    } else {
        std::fs::write(&config_path, default_config_yaml(args.strict_identity))
            .map_err(|e| CliError::internal(COMMAND, e))?;
        report.created.push(".firetrail/config.yml".into());
    }

    // 3. identity.yml stub — only if absent.
    let identity_path = ws.identity_path();
    if identity_path.exists() {
        report.preserved.push(".firetrail/identity.yml".into());
    } else {
        std::fs::write(&identity_path, default_identity_yaml())
            .map_err(|e| CliError::internal(COMMAND, e))?;
        report.created.push(".firetrail/identity.yml".into());
    }

    // 4. Index DB — create / open is idempotent.
    {
        let _index = Index::open(&ws.root).map_err(|e| CliError::internal(COMMAND, e))?;
    }
    track(
        &mut report,
        &ws.index_db_path(),
        ".firetrail/index.db",
        fresh,
    );

    // 5. .gitignore additions.
    update_gitignore(&ws.root, &mut report)?;

    // 6. Hooks.
    if !args.no_hooks {
        let repo = Repo::open(&ws.root).map_err(|e| CliError::internal(COMMAND, e))?;
        for (hook, body) in default_hooks() {
            repo.install_hook(hook, body)
                .map_err(|e| CliError::internal(COMMAND, e))?;
            report.hooks_installed.push(hook.filename().to_string());
        }
    }

    // 7. AGENTS.md / .claude/skills/firetrail/SKILL.md.
    if !args.no_agents {
        write_if_absent(
            &ws.root.join("AGENTS.md"),
            &default_agents_md(),
            "AGENTS.md",
            &mut report,
        )?;
        let skill_dir = ws.root.join(".claude/skills/firetrail");
        std::fs::create_dir_all(&skill_dir).map_err(|e| CliError::internal(COMMAND, e))?;
        write_if_absent(
            &skill_dir.join("SKILL.md"),
            &default_skill_md(),
            ".claude/skills/firetrail/SKILL.md",
            &mut report,
        )?;
    }

    // Quiet flag is honoured at the formatter layer; verbose flag enables
    // tracing; nothing else to do here.
    let _ = global;

    Ok(CommandOutcome::Init(report))
}

fn track(report: &mut InitReport, path: &Path, label: &str, fresh: bool) {
    if fresh && path.exists() {
        report.created.push(label.to_string());
    } else if path.exists() {
        report.preserved.push(label.to_string());
    }
}

fn write_if_absent(
    path: &Path,
    content: &str,
    label: &str,
    report: &mut InitReport,
) -> Result<(), CliError> {
    if path.exists() {
        report.preserved.push(label.to_string());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::internal(COMMAND, e))?;
    }
    std::fs::write(path, content).map_err(|e| CliError::internal(COMMAND, e))?;
    report.created.push(label.to_string());
    Ok(())
}

fn update_gitignore(root: &Path, report: &mut InitReport) -> Result<(), CliError> {
    let path = root.join(".gitignore");
    let existing = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| CliError::internal(COMMAND, e))?
    } else {
        String::new()
    };

    let wanted = [".firetrail/index.db", ".firetrail/cache/"];
    let missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|entry| {
            !existing
                .lines()
                .any(|line| line.trim() == *entry || line.trim() == format!("/{entry}"))
        })
        .collect();

    if missing.is_empty() {
        report.preserved.push(".gitignore".into());
        return Ok(());
    }

    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    if !new_content.contains("# firetrail") {
        new_content.push_str("\n# firetrail\n");
    }
    for entry in missing {
        new_content.push_str(entry);
        new_content.push('\n');
    }
    std::fs::write(&path, new_content).map_err(|e| CliError::internal(COMMAND, e))?;
    report.created.push(".gitignore".into());
    Ok(())
}

fn default_config_yaml(strict_identity: bool) -> String {
    let strict = if strict_identity { "true" } else { "false" };
    format!(
        "# Firetrail workspace config\n\
         format_version: 1\n\
         storage:\n  mode: embedded\n\
         identity:\n  strict: {strict}\n\
         claim:\n  default_duration: 7d\n"
    )
}

fn default_identity_yaml() -> String {
    "# .firetrail/identity.yml — local identity override (optional)\n\
     # name: your.name@example.com\n"
        .to_string()
}

fn default_agents_md() -> String {
    "# AGENTS.md\n\n\
     This repository uses [Firetrail](https://github.com/firetrail/firetrail) for the work graph and memory layer.\n\n\
     Agents working in this repo should:\n\n\
     - Discover work via `firetrail ready` (M1+).\n\
     - Claim a task via `firetrail claim <id>` before starting.\n\
     - Record findings via `firetrail finding create` (M2+).\n\
     - Run `firetrail doctor` to verify the workspace is healthy.\n"
        .to_string()
}

fn default_skill_md() -> String {
    "---\n\
     name: firetrail\n\
     description: Use the `firetrail` CLI for work-graph queries and updates.\n\
     ---\n\n\
     # Firetrail skill (M1 stub)\n\n\
     Run `firetrail --help` for the full command surface. Key commands:\n\n\
     - `firetrail ready` — list work ready to pick up.\n\
     - `firetrail show <id>` — inspect a single record.\n\
     - `firetrail doctor` — verify workspace health.\n\n\
     This skill is intentionally minimal in M1; M2 ships the full agent skill.\n"
        .to_string()
}

fn default_hooks() -> [(HookName, &'static str); 3] {
    [
        (
            HookName::PreCommit,
            "# firetrail pre-commit: protect history & validate records.\n\
             # Real implementation arrives with ft-pr / ft-history (M4).\n\
             exit 0\n",
        ),
        (
            HookName::PostCheckout,
            "# firetrail post-checkout: refresh local index after branch switch.\n\
             # Real implementation arrives with ft-index refresh hook (M2).\n\
             exit 0\n",
        ),
        (
            HookName::PostMerge,
            "# firetrail post-merge: refresh local index after a merge.\n\
             # Real implementation arrives with ft-index refresh hook (M2).\n\
             exit 0\n",
        ),
    ]
}
