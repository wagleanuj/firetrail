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
use ft_scope::ScopeRegistry;
use ft_storage::{profile_get, profile_get_base, profile_get_for_scope, profile_list};
use serde::Serialize;

use crate::cli::{
    GlobalOpts, ProfileComponentAddArgs, ProfileComponentRmArgs, ProfileListArgs,
    ProfileResolveArgs, ProfileSetArgs, ProfileShowArgs,
};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const SHOW: &str = "profile show";
const SET: &str = "profile set";
const LIST: &str = "profile list";
const RESOLVE: &str = "profile resolve";
const COMPONENT_ADD: &str = "profile component add";
const COMPONENT_RM: &str = "profile component rm";

/// Scope label used for the base profile (no `owning_scope`).
const BASE_SCOPE_LABEL: &str = "(base)";

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

/// One row of `firetrail profile list`: a base or per-scope profile record.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileListRow {
    /// Owning scope id, or `(base)` for the base profile.
    pub scope: String,
    /// Canonical record id.
    pub id: String,
    /// Whether the (stored, unmerged) record carries a validate command.
    pub has_validate: bool,
}

/// Outcome of `firetrail profile list` — the base profile + every scope delta.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileListOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// One row per `RepoProfile` record (base + scopes).
    pub profiles: Vec<ProfileListRow>,
    /// Non-fatal warnings to surface in the JSON envelope.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ProfileListOutcome {
    /// Markdown table of the rows.
    #[must_use]
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::from("# repo profiles\n\n| Scope | Id | Validate |\n|---|---|---|\n");
        for row in &self.profiles {
            let _ = writeln!(
                s,
                "| {} | `{}` | {} |",
                row.scope,
                row.id,
                if row.has_validate { "yes" } else { "no" }
            );
        }
        s
    }

    /// One-line summary for `--quiet`.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("profile list: {} profile(s)", self.profiles.len())
    }

    /// JSON payload.
    #[must_use]
    pub fn json_data(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// `firetrail profile list` — one row per `RepoProfile` record (base + scopes).
pub fn list(_args: &ProfileListArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(LIST, global.workspace.as_deref())?;
    let records =
        profile_list(&ctx.storage).map_err(|e| CliError::internal(LIST, format!("list: {e}")))?;
    let profiles = records
        .into_iter()
        .map(|r| {
            let scope = r
                .envelope
                .owning_scope
                .clone()
                .unwrap_or_else(|| BASE_SCOPE_LABEL.to_string());
            let has_validate = match &r.body {
                RecordBody::RepoProfile(b) => b.validate_command.is_some(),
                _ => false,
            };
            ProfileListRow {
                scope,
                id: r.envelope.id.as_str().to_string(),
                has_validate,
            }
        })
        .collect();
    Ok(CommandOutcome::ProfileList(ProfileListOutcome {
        command: LIST,
        profiles,
        warnings: ctx.warnings.clone(),
    }))
}

/// One distinct validate command in a resolve plan (serializable mirror of
/// [`ft_ops::profile::resolve::ValidateEntry`]).
#[derive(Debug, Clone, Serialize)]
pub struct ResolveEntry {
    /// The validate command to run.
    pub command: String,
    /// Scope ids (sorted, unique) that resolved to this command. Empty = base.
    pub scopes: Vec<String>,
    /// How many changed files resolved to this command.
    pub file_count: usize,
}

/// Outcome of `firetrail profile resolve` — a serializable [`ValidatePlan`].
#[derive(Debug, Clone, Serialize)]
pub struct ProfileResolveOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Distinct validate commands, ordered by command string.
    pub entries: Vec<ResolveEntry>,
    /// Changed files whose resolved profile has no validate command.
    pub unresolved: usize,
    /// Total number of changed paths considered.
    pub path_count: usize,
    /// Non-fatal warnings to surface in the JSON envelope.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ProfileResolveOutcome {
    /// Markdown table of the distinct commands + unresolved count.
    #[must_use]
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# validate plan\n\n{} path(s), {} distinct command(s), {} unresolved\n\n",
            self.path_count,
            self.entries.len(),
            self.unresolved
        );
        if !self.entries.is_empty() {
            s.push_str("| Command | Scopes | Files |\n|---|---|---|\n");
            for e in &self.entries {
                let scopes = if e.scopes.is_empty() {
                    BASE_SCOPE_LABEL.to_string()
                } else {
                    e.scopes.join(", ")
                };
                let _ = writeln!(s, "| `{}` | {} | {} |", e.command, scopes, e.file_count);
            }
        }
        s
    }

    /// One-line summary for `--quiet`.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!(
            "profile resolve: {} command(s), {} unresolved",
            self.entries.len(),
            self.unresolved
        )
    }

    /// JSON payload.
    #[must_use]
    pub fn json_data(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// `firetrail profile resolve` — resolve a changeset to its distinct validate
/// commands. Paths come from explicit `--paths`, the staged git diff
/// (`--staged`), or the diff against a ref (`--base <ref>`).
pub fn resolve(args: &ProfileResolveArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(RESOLVE, global.workspace.as_deref())?;
    let paths = gather_paths(&ctx, args)?;

    let reg = ScopeRegistry::load(&ctx.ws.root)
        .map_err(|e| CliError::internal(RESOLVE, format!("load scope registry: {e}")))?;
    let base = profile_get_base(&ctx.storage)
        .map_err(|e| CliError::internal(RESOLVE, format!("read base profile: {e}")))?
        .map(|r| body_or_default(Some(&r)))
        .unwrap_or_default();

    let plan = ft_ops::profile::resolve::validate_plan(&reg, &base, &paths, |id| {
        profile_get_for_scope(&ctx.storage, id)
            .ok()
            .flatten()
            .and_then(|r| match r.body {
                RecordBody::RepoProfile(b) => Some(b),
                _ => None,
            })
    });

    let entries = plan
        .entries
        .into_iter()
        .map(|e| ResolveEntry {
            command: e.command,
            scopes: e.scopes,
            file_count: e.file_count,
        })
        .collect();

    Ok(CommandOutcome::ProfileResolve(ProfileResolveOutcome {
        command: RESOLVE,
        entries,
        unresolved: plan.unresolved,
        path_count: paths.len(),
        warnings: ctx.warnings.clone(),
    }))
}

/// Gather the changed paths for `resolve`: explicit `--paths`, the staged git
/// diff (`--staged`), or the diff between `--base <ref>` and HEAD.
fn gather_paths(
    ctx: &WorkCtx,
    args: &ProfileResolveArgs,
) -> Result<Vec<std::path::PathBuf>, CliError> {
    if !args.paths.is_empty() {
        return Ok(args.paths.clone());
    }
    if args.staged {
        let git = ft_git::Repo::open(&ctx.ws.root)
            .map_err(|e| CliError::internal(RESOLVE, format!("open git: {e}")))?;
        let status = git
            .status()
            .map_err(|e| CliError::internal(RESOLVE, format!("git status: {e}")))?;
        return Ok(status.staged);
    }
    if let Some(base) = &args.base {
        let git = ft_git::Repo::open(&ctx.ws.root)
            .map_err(|e| CliError::internal(RESOLVE, format!("open git: {e}")))?;
        let entries = git
            .diff(base, "HEAD", None)
            .map_err(|e| CliError::internal(RESOLVE, format!("git diff {base}..HEAD: {e}")))?;
        return Ok(entries.into_iter().map(|e| e.path).collect());
    }
    Err(CliError::user(
        RESOLVE,
        "no paths to resolve; pass --paths, --staged, or --base <ref>".to_string(),
    ))
}

/// Validate that `scope_id` is a declared scope in `.firetrail/scopes.yaml`.
///
/// Loads the [`ScopeRegistry`] for the workspace and errors with
/// [`CliError::user`] when the id is not a known scope — so a scoped
/// `set` / `show` / `component` never writes against a typo'd or dangling
/// scope.
fn require_scope(ctx: &WorkCtx, command: &'static str, scope_id: &str) -> Result<(), CliError> {
    let registry = ScopeRegistry::load(&ctx.ws.root)
        .map_err(|e| CliError::internal(command, format!("load scope registry: {e}")))?;
    if registry.get(scope_id).is_none() {
        return Err(CliError::user(
            command,
            format!("unknown scope `{scope_id}`; declare it in `.firetrail/scopes.yaml` first"),
        ));
    }
    Ok(())
}

/// Persist `body` as the per-scope delta for `scope_id` — update the existing
/// scoped record in place, or create a fresh `Draft` scoped record stamped with
/// `owning_scope = Some(scope_id)`. Mirrors [`persist`] but keyed on the scope.
fn persist_scope(
    ctx: &mut WorkCtx,
    command: &'static str,
    scope_id: &str,
    existing: Option<Record>,
    mut body: RepoProfileBody,
) -> Result<Record, CliError> {
    body.trust = TrustState::Draft;
    if let Some(mut record) = existing {
        record.body = RecordBody::RepoProfile(body);
        record.envelope.updated_at = chrono::Utc::now();
        ctx.save_record(&mut record)?;
        Ok(record)
    } else {
        let actor = ctx.actor()?;
        let mut record = RecordBuilder::new(RecordKind::RepoProfile, PROFILE_TITLE, actor)
            .owning_scope(scope_id)
            .repo_profile(body)
            .build()
            .map_err(|e| CliError::internal(command, format!("build profile: {e}")))?;
        ctx.save_record(&mut record)?;
        Ok(record)
    }
}

/// `firetrail profile show` — print the current profile; error if absent.
///
/// With `--scope <id>` the per-scope delta is shown instead of the base. Adding
/// `--resolved` renders the merged view (base ⊕ delta). Without `--scope` the
/// behaviour is byte-identical to before (the singleton base profile).
pub fn show(args: &ProfileShowArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(SHOW, global.workspace.as_deref())?;

    let Some(scope_id) = args.scope.as_deref() else {
        // Base path — unchanged from the singleton surface.
        let Some(record) = profile_get(&ctx.storage)
            .map_err(|e| CliError::internal(SHOW, format!("read profile: {e}")))?
        else {
            return Err(CliError::NotFound {
                command: SHOW.to_string(),
                what: "repo profile".to_string(),
            });
        };
        return Ok(CommandOutcome::Profile(ProfileOutcome::new(
            SHOW,
            record,
            ctx.warnings.clone(),
        )));
    };

    require_scope(&ctx, SHOW, scope_id)?;
    let Some(mut record) = profile_get_for_scope(&ctx.storage, scope_id)
        .map_err(|e| CliError::internal(SHOW, format!("read scope profile: {e}")))?
    else {
        return Err(CliError::NotFound {
            command: SHOW.to_string(),
            what: format!("repo profile for scope `{scope_id}`"),
        });
    };

    if args.resolved {
        let base_body = profile_get_base(&ctx.storage)
            .map_err(|e| CliError::internal(SHOW, format!("read base profile: {e}")))?
            .map(|r| body_or_default(Some(&r)))
            .unwrap_or_default();
        let delta_body = body_or_default(Some(&record));
        record.body =
            RecordBody::RepoProfile(ft_ops::profile::resolve::merge(&base_body, &delta_body));
    }

    Ok(CommandOutcome::Profile(ProfileOutcome::new(
        SHOW,
        record,
        ctx.warnings.clone(),
    )))
}

/// `firetrail profile set` — create-if-absent, else partial update in place.
pub fn set(args: &ProfileSetArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(SET, global.workspace.as_deref())?;

    // Read the record being edited: the per-scope delta when `--scope` is given
    // (validated first), else the singleton base.
    let existing = if let Some(scope_id) = args.scope.as_deref() {
        require_scope(&ctx, SET, scope_id)?;
        profile_get_for_scope(&ctx.storage, scope_id)
            .map_err(|e| CliError::internal(SET, format!("read scope profile: {e}")))?
    } else {
        profile_get(&ctx.storage)
            .map_err(|e| CliError::internal(SET, format!("read profile: {e}")))?
    };
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

    let record = match args.scope.as_deref() {
        Some(scope_id) => persist_scope(&mut ctx, SET, scope_id, existing, body)?,
        None => persist(&mut ctx, SET, existing, body)?,
    };
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
    let existing = if let Some(scope_id) = args.scope.as_deref() {
        require_scope(&ctx, COMPONENT_ADD, scope_id)?;
        profile_get_for_scope(&ctx.storage, scope_id)
            .map_err(|e| CliError::internal(COMPONENT_ADD, format!("read scope profile: {e}")))?
    } else {
        profile_get(&ctx.storage)
            .map_err(|e| CliError::internal(COMPONENT_ADD, format!("read profile: {e}")))?
    };
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

    let record = match args.scope.as_deref() {
        Some(scope_id) => persist_scope(&mut ctx, COMPONENT_ADD, scope_id, existing, body)?,
        None => persist(&mut ctx, COMPONENT_ADD, existing, body)?,
    };
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
    let existing = if let Some(scope_id) = args.scope.as_deref() {
        require_scope(&ctx, COMPONENT_RM, scope_id)?;
        profile_get_for_scope(&ctx.storage, scope_id)
            .map_err(|e| CliError::internal(COMPONENT_RM, format!("read scope profile: {e}")))?
    } else {
        profile_get(&ctx.storage)
            .map_err(|e| CliError::internal(COMPONENT_RM, format!("read profile: {e}")))?
    };
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

    let record = match args.scope.as_deref() {
        Some(scope_id) => persist_scope(&mut ctx, COMPONENT_RM, scope_id, existing, body)?,
        None => persist(&mut ctx, COMPONENT_RM, existing, body)?,
    };
    Ok(CommandOutcome::Profile(ProfileOutcome::new(
        COMPONENT_RM,
        record,
        ctx.warnings.clone(),
    )))
}
