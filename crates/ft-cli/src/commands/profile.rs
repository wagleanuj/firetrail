//! `firetrail profile {show,set,component {add,rm}}` — the singleton repo
//! profile surface.
//!
//! The repo profile is a single `RepoProfile` record holding the canonical
//! validate / test / build / lint commands, language + tooling facts, and a
//! shallow component map (ADR-0005: the agent decides the contents; firetrail
//! only stores them). `set` and `component` apply **partial updates** and
//! always write the record as `Draft` — confirmation (Draft → Reviewed →
//! Verified) is a separate `firetrail trust` transition.
//!
//! Reads go through [`ft_storage::profile_get`]; writes go through the
//! [`WorkCtx::save_record`] choke point so external-mode auto-commit, index
//! refresh, and per-record history all apply uniformly.
//!
//! Design: `docs/specs/2026-05-31-repo-profile-bootstrap-design.md` §2.

use ft_core::{
    ComponentRef, Record, RecordBody, RecordBuilder, RecordKind, RepoProfileBody, TrustState,
};
use ft_storage::profile_get;
use serde::Serialize;

use crate::cli::{
    GlobalOpts, ProfileComponentAddArgs, ProfileComponentRmArgs, ProfileSetArgs, ProfileShowArgs,
};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const SHOW: &str = "profile show";
const SET: &str = "profile set";
const COMPONENT_ADD: &str = "profile component add";
const COMPONENT_RM: &str = "profile component rm";

/// Title given to a freshly-created repo profile record. Matches the constant
/// used by `ft_storage::profile_set` so the singleton reads identically.
const PROFILE_TITLE: &str = "Repo profile";

/// Outcome of any `profile` subcommand: carries the full profile record so the
/// `--json` path can serialize the whole envelope + body.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileOutcome {
    /// Stable command name (e.g. `"profile set"`).
    #[serde(skip)]
    pub command: &'static str,
    /// The repo profile record (envelope + `RepoProfileBody`).
    pub record: Record,
    /// Non-fatal warnings to surface in the JSON envelope.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ProfileOutcome {
    fn new(command: &'static str, record: Record, warnings: Vec<String>) -> Self {
        Self {
            command,
            record,
            warnings,
        }
    }

    /// Borrow the profile body out of the record.
    fn body(&self) -> Option<&RepoProfileBody> {
        match &self.record.body {
            RecordBody::RepoProfile(b) => Some(b),
            _ => None,
        }
    }

    /// Human-readable table of commands, tooling facts, and components.
    #[must_use]
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let Some(b) = self.body() else {
            return format!("**{}** — (not a repo profile record)\n", self.command);
        };
        let mut s = format!(
            "# repo profile\n\n`{}` — trust **{:?}**\n\n## Commands\n\n| Command | Value |\n|---|---|\n",
            self.record.envelope.id, b.trust
        );
        let row = |label: &str, v: &Option<String>| {
            format!("| {label} | {} |\n", v.as_deref().unwrap_or("—"))
        };
        s.push_str(&row("validate", &b.validate_command));
        s.push_str(&row("test", &b.test_command));
        s.push_str(&row("build", &b.build_command));
        s.push_str(&row("lint", &b.lint_command));

        let _ = write!(
            s,
            "\n## Tooling\n\n- languages: {}\n- package managers: {}\n- runtime: {}\n",
            if b.languages.is_empty() {
                "—".to_string()
            } else {
                b.languages.join(", ")
            },
            if b.package_managers.is_empty() {
                "—".to_string()
            } else {
                b.package_managers.join(", ")
            },
            b.runtime.as_deref().unwrap_or("—"),
        );

        s.push_str("\n## Components\n\n");
        if b.components.is_empty() {
            s.push_str("(none)\n");
        } else {
            s.push_str("| Name | Path | Summary |\n|---|---|---|\n");
            for c in &b.components {
                let _ = writeln!(
                    s,
                    "| {} | `{}` | {} |",
                    c.name,
                    c.path,
                    c.summary.as_deref().unwrap_or("—")
                );
            }
        }
        if let Some(note) = &b.notes {
            let _ = write!(s, "\n## Notes\n\n{note}\n");
        }
        s
    }

    /// One-line summary for `--quiet`.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        let trust = self.body().map_or(TrustState::Draft, |b| b.trust);
        format!("{} {} ({trust:?})", self.command, self.record.envelope.id)
    }

    /// JSON payload (the full record).
    #[must_use]
    pub fn json_data(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Read the profile body out of a record, or a fresh `Draft` body when absent.
fn body_or_default(existing: Option<&Record>) -> RepoProfileBody {
    match existing.map(|r| &r.body) {
        Some(RecordBody::RepoProfile(b)) => b.clone(),
        _ => RepoProfileBody::default(),
    }
}

/// Persist `body` as the singleton profile — update the existing record in
/// place, or create a fresh `Draft` record. Routes through
/// [`WorkCtx::save_record`] so external-mode commit + index refresh apply.
fn persist(
    ctx: &mut WorkCtx,
    command: &'static str,
    existing: Option<Record>,
    mut body: RepoProfileBody,
) -> Result<Record, CliError> {
    // The agent proposes as Draft; confirmation is a separate trust transition.
    body.trust = TrustState::Draft;
    if let Some(mut record) = existing {
        record.body = RecordBody::RepoProfile(body);
        record.envelope.updated_at = chrono::Utc::now();
        ctx.save_record(&mut record)?;
        Ok(record)
    } else {
        let actor = ctx.actor()?;
        let mut record = RecordBuilder::new(RecordKind::RepoProfile, PROFILE_TITLE, actor)
            .repo_profile(body)
            .build()
            .map_err(|e| CliError::internal(command, format!("build profile: {e}")))?;
        ctx.save_record(&mut record)?;
        Ok(record)
    }
}

/// `firetrail profile show` — print the current profile; error if absent.
pub fn show(_args: &ProfileShowArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(SHOW, global.workspace.as_deref())?;
    let Some(record) = profile_get(&ctx.storage)
        .map_err(|e| CliError::internal(SHOW, format!("read profile: {e}")))?
    else {
        return Err(CliError::NotFound {
            command: SHOW.to_string(),
            what: "repo profile".to_string(),
        });
    };
    Ok(CommandOutcome::Profile(ProfileOutcome::new(
        SHOW,
        record,
        ctx.warnings.clone(),
    )))
}

/// `firetrail profile set` — create-if-absent, else partial update in place.
pub fn set(args: &ProfileSetArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(SET, global.workspace.as_deref())?;
    let existing = profile_get(&ctx.storage)
        .map_err(|e| CliError::internal(SET, format!("read profile: {e}")))?;
    let mut body = body_or_default(existing.as_ref());

    // Option flags overwrite when Some; otherwise the stored value is kept.
    if args.validate.is_some() {
        body.validate_command.clone_from(&args.validate);
    }
    if args.test.is_some() {
        body.test_command.clone_from(&args.test);
    }
    if args.build.is_some() {
        body.build_command.clone_from(&args.build);
    }
    if args.lint.is_some() {
        body.lint_command.clone_from(&args.lint);
    }
    if args.runtime.is_some() {
        body.runtime.clone_from(&args.runtime);
    }
    if args.note.is_some() {
        body.notes.clone_from(&args.note);
    }
    // Repeatable vec flags overwrite only when at least one value was given.
    if !args.languages.is_empty() {
        body.languages.clone_from(&args.languages);
    }
    if !args.package_managers.is_empty() {
        body.package_managers.clone_from(&args.package_managers);
    }

    let record = persist(&mut ctx, SET, existing, body)?;
    Ok(CommandOutcome::Profile(ProfileOutcome::new(
        SET,
        record,
        ctx.warnings.clone(),
    )))
}

/// `firetrail profile component add <name> <path>` — add / update a component.
pub fn component_add(
    args: &ProfileComponentAddArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMPONENT_ADD, global.workspace.as_deref())?;
    let existing = profile_get(&ctx.storage)
        .map_err(|e| CliError::internal(COMPONENT_ADD, format!("read profile: {e}")))?;
    let mut body = body_or_default(existing.as_ref());

    let new_ref = ComponentRef {
        name: args.name.clone(),
        path: args.path.clone(),
        summary: args.summary.clone(),
    };
    // Update-in-place when a component with the same name already exists,
    // otherwise append. Keeps the map a set keyed by name.
    if let Some(slot) = body.components.iter_mut().find(|c| c.name == args.name) {
        *slot = new_ref;
    } else {
        body.components.push(new_ref);
    }

    let record = persist(&mut ctx, COMPONENT_ADD, existing, body)?;
    Ok(CommandOutcome::Profile(ProfileOutcome::new(
        COMPONENT_ADD,
        record,
        ctx.warnings.clone(),
    )))
}

/// `firetrail profile component rm <name>` — remove a component by name.
pub fn component_rm(
    args: &ProfileComponentRmArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMPONENT_RM, global.workspace.as_deref())?;
    let existing = profile_get(&ctx.storage)
        .map_err(|e| CliError::internal(COMPONENT_RM, format!("read profile: {e}")))?;
    let Some(_) = existing.as_ref() else {
        return Err(CliError::NotFound {
            command: COMPONENT_RM.to_string(),
            what: "repo profile".to_string(),
        });
    };
    let mut body = body_or_default(existing.as_ref());

    let before = body.components.len();
    body.components.retain(|c| c.name != args.name);
    if body.components.len() == before {
        return Err(CliError::user(
            COMPONENT_RM,
            format!("no component named `{}` in the profile", args.name),
        ));
    }

    let record = persist(&mut ctx, COMPONENT_RM, existing, body)?;
    Ok(CommandOutcome::Profile(ProfileOutcome::new(
        COMPONENT_RM,
        record,
        ctx.warnings.clone(),
    )))
}
