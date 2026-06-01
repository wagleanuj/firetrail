//! `firetrail scope …` — scope registry queries (M5).
//!
//! Loads `.firetrail/scopes.yaml` via [`ft_scope::ScopeRegistry`] and surfaces
//! list / show / aliases / owners views.

use std::fmt::Write as _;

use ft_scope::writer::{load_file, remove_scope, reorder, save_file, upsert_scope};
use ft_scope::{ScopeRegistry, ScopeYaml, ScopesFile, load};
use serde::Serialize;

use crate::cli::{
    GlobalOpts, ScopeAddArgs, ScopeEditArgs, ScopeOwnersArgs, ScopeReorderArgs, ScopeRmArgs,
    ScopeShowArgs,
};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_LIST: &str = "scope list";
const CMD_SHOW: &str = "scope show";
const CMD_ALIASES: &str = "scope aliases";
const CMD_OWNERS: &str = "scope owners";
const CMD_ADD: &str = "scope add";
const CMD_EDIT: &str = "scope edit";
const CMD_RM: &str = "scope rm";
const CMD_REORDER: &str = "scope reorder";

/// Outcome of `scope list`.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeListOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Each loaded scope.
    pub scopes: Vec<ScopeSummary>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of `scope show`.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeShowOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// The scope.
    pub scope: ScopeDetail,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of `scope aliases`.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeAliasesOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Alias → scope id entries (alphabetical by alias).
    pub aliases: Vec<AliasEntry>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of `scope owners`.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeOwnersOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Resolved path (string form).
    pub path: String,
    /// Identities resolved by CODEOWNERS.
    pub owners: Vec<String>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of a scope *write* (`add` / `edit` / `rm` / `reorder`).
///
/// Carries the full, post-write scopes list (declaration order preserved) so
/// the `--json` path can serialize the new file state, plus the affected scope
/// id for the human / quiet rendering.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeWriteOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Verb performed (e.g. `added`, `updated`, `removed`, `reordered`).
    pub action: &'static str,
    /// The scope id(s) the write touched (single id for add/edit/rm; the full
    /// ordered list for reorder).
    pub targets: Vec<String>,
    /// The full scopes list after the write, in declaration order.
    pub scopes: Vec<ScopeYamlView>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Serializable mirror of [`ft_scope::ScopeYaml`] for write outcomes.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeYamlView {
    /// Canonical id.
    pub id: String,
    /// Display name (if set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `applies_to` glob patterns.
    pub applies_to: Vec<String>,
    /// Declared aliases.
    pub aliases: Vec<String>,
    /// CODEOWNERS path (if set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codeowners: Option<String>,
}

impl From<&ScopeYaml> for ScopeYamlView {
    fn from(s: &ScopeYaml) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            applies_to: s.applies_to.clone(),
            aliases: s.aliases.clone(),
            codeowners: s.codeowners.as_ref().map(|p| p.display().to_string()),
        }
    }
}

impl ScopeWriteOutcome {
    fn new(
        command: &'static str,
        action: &'static str,
        targets: Vec<String>,
        file: &ScopesFile,
    ) -> Self {
        Self {
            command,
            action,
            targets,
            scopes: file.scopes.iter().map(ScopeYamlView::from).collect(),
            warnings: Vec::new(),
        }
    }

    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut s = format!(
            "**scope {}** {} — {} scope(s) configured\n",
            self.action,
            self.targets.join(", "),
            self.scopes.len()
        );
        if !self.scopes.is_empty() {
            s.push_str("\n| id | applies_to | aliases |\n|---|---|---|\n");
            for sc in &self.scopes {
                let _ = writeln!(
                    s,
                    "| `{}` | {} | {} |",
                    sc.id,
                    sc.applies_to.join(", "),
                    if sc.aliases.is_empty() {
                        "—".into()
                    } else {
                        sc.aliases.join(", ")
                    },
                );
            }
        }
        s
    }

    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("scope {} {}", self.action, self.targets.join(", "))
    }

    /// JSON payload.
    #[must_use]
    pub fn json_data(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Summary view of a single scope.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeSummary {
    /// Canonical id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `applies_to` glob patterns.
    pub applies_to: Vec<String>,
    /// Declared aliases (excluding the implicit self-alias).
    pub aliases: Vec<String>,
    /// Whether a CODEOWNERS file is wired to this scope.
    pub has_codeowners: bool,
}

/// Detail view of a single scope.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeDetail {
    /// Summary fields.
    #[serde(flatten)]
    pub summary: ScopeSummary,
    /// Codeowners patterns (raw glob → owner ids).
    pub codeowners: Vec<CodeOwnersRow>,
}

/// A single codeowners line.
#[derive(Debug, Clone, Serialize)]
pub struct CodeOwnersRow {
    /// Raw glob pattern.
    pub pattern: String,
    /// Owners (identity strings, as they appear in the CODEOWNERS file).
    pub owners: Vec<String>,
}

/// Alias → scope id entry.
#[derive(Debug, Clone, Serialize)]
pub struct AliasEntry {
    /// Alias text.
    pub alias: String,
    /// Scope id the alias resolves to.
    pub scope_id: String,
}

impl ScopeListOutcome {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        if self.scopes.is_empty() {
            return "_no scopes configured_\n".into();
        }
        let mut s = String::from("| id | name | applies_to | codeowners |\n|---|---|---|---|\n");
        for sc in &self.scopes {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} |",
                sc.id,
                sc.name,
                sc.applies_to.join(", "),
                if sc.has_codeowners { "yes" } else { "—" },
            );
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("scope list ({})", self.scopes.len())
    }
}

impl ScopeShowOutcome {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        let sc = &self.scope.summary;
        let mut s = format!(
            "**scope** `{}` ({})\n\n- applies_to: {}\n- aliases: {}\n",
            sc.id,
            sc.name,
            sc.applies_to.join(", "),
            if sc.aliases.is_empty() {
                "_none_".into()
            } else {
                sc.aliases.join(", ")
            }
        );
        if !self.scope.codeowners.is_empty() {
            s.push_str("\n## CODEOWNERS\n\n");
            for row in &self.scope.codeowners {
                let _ = writeln!(s, "- `{}` → {}", row.pattern, row.owners.join(", "));
            }
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("scope show {}", self.scope.summary.id)
    }
}

impl ScopeAliasesOutcome {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        if self.aliases.is_empty() {
            return "_no scope aliases configured_\n".into();
        }
        let mut s = String::from("| alias | scope |\n|---|---|\n");
        for a in &self.aliases {
            let _ = writeln!(s, "| `{}` | `{}` |", a.alias, a.scope_id);
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("scope aliases ({})", self.aliases.len())
    }
}

impl ScopeOwnersOutcome {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        if self.owners.is_empty() {
            return format!("_no owners for `{}`_\n", self.path);
        }
        let mut s = format!("**owners for `{}`**\n", self.path);
        for o in &self.owners {
            let _ = writeln!(s, "- `{o}`");
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("scope owners {} ({})", self.path, self.owners.len())
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────

fn open_registry(command: &str, global: &GlobalOpts) -> Result<ScopeRegistry, CliError> {
    let ws = crate::workspace::require_initialised(command, global.workspace.as_deref())?;
    load(&ws.root).map_err(|e| CliError::internal(command, format!("load scopes: {e}")))
}

fn summary_of(s: &ft_scope::Scope) -> ScopeSummary {
    ScopeSummary {
        id: s.id.clone(),
        name: s.name.clone(),
        applies_to: s.applies_to_patterns.clone(),
        aliases: s.aliases.clone(),
        has_codeowners: s.codeowners.is_some(),
    }
}

/// `firetrail scope list`
pub fn list(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let registry = open_registry(CMD_LIST, global)?;
    let scopes: Vec<ScopeSummary> = registry.scopes().iter().map(summary_of).collect();
    Ok(CommandOutcome::ScopeList(ScopeListOutcome {
        command: CMD_LIST,
        scopes,
        warnings: Vec::new(),
    }))
}

/// `firetrail scope show`
pub fn show(args: &ScopeShowArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let registry = open_registry(CMD_SHOW, global)?;
    let scope = registry
        .get(&args.id)
        .or_else(|| registry.resolve_alias(&args.id))
        .ok_or_else(|| CliError::NotFound {
            command: CMD_SHOW.into(),
            what: args.id.clone(),
        })?;
    let codeowners = scope
        .codeowners
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|e| CodeOwnersRow {
                    pattern: e.pattern.clone(),
                    owners: e.owners.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let detail = ScopeDetail {
        summary: summary_of(scope),
        codeowners,
    };
    Ok(CommandOutcome::ScopeShow(ScopeShowOutcome {
        command: CMD_SHOW,
        scope: detail,
        warnings: Vec::new(),
    }))
}

/// `firetrail scope aliases`
pub fn aliases(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let registry = open_registry(CMD_ALIASES, global)?;
    let mut entries: Vec<AliasEntry> = Vec::new();
    for sc in registry.scopes() {
        for alias in &sc.aliases {
            entries.push(AliasEntry {
                alias: alias.clone(),
                scope_id: sc.id.clone(),
            });
        }
        // Include the implicit self-alias unless one of the declared
        // aliases already covers it.
        if !sc.aliases.iter().any(|a| a == &sc.id) {
            entries.push(AliasEntry {
                alias: sc.id.clone(),
                scope_id: sc.id.clone(),
            });
        }
    }
    entries.sort_by(|a, b| a.alias.cmp(&b.alias));
    Ok(CommandOutcome::ScopeAliases(ScopeAliasesOutcome {
        command: CMD_ALIASES,
        aliases: entries,
        warnings: Vec::new(),
    }))
}

/// `firetrail scope owners`
pub fn owners(args: &ScopeOwnersArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let registry = open_registry(CMD_OWNERS, global)?;
    let resolved = registry.owners_for_path(&args.path);
    let owners: Vec<String> = resolved.iter().map(|i| i.as_str().to_string()).collect();
    Ok(CommandOutcome::ScopeOwners(ScopeOwnersOutcome {
        command: CMD_OWNERS,
        path: args.path.display().to_string(),
        owners,
        warnings: Vec::new(),
    }))
}

// ── Write handlers ───────────────────────────────────────────────────────────
//
// The write path operates on the raw [`ScopesFile`] / [`ScopeYaml`] model via
// the `ft_scope::writer` API: `load_file` (absent → empty), mutate, then
// `save_file` (validates dup id/alias, glob compile, empty applies_to before
// writing). Validation errors are surfaced as `CliError::user` with the
// writer's own message. We open the workspace via `WorkCtx::open` to get
// `ctx.ws.root` — load_file / save_file are pure filesystem operations and do
// not auto-create the file unless a write is requested (zero-overhead).

/// `firetrail scope add` — append a new scope (becomes last-declared).
pub fn add(args: &ScopeAddArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_ADD, global.workspace.as_deref())?;
    let mut file = load_file(&ctx.ws.root)
        .map_err(|e| CliError::internal(CMD_ADD, format!("load scopes: {e}")))?;

    // `add` is create-only: refuse to clobber an existing scope (use `edit` to
    // change one). `upsert_scope` would silently replace in place.
    if file.scopes.iter().any(|s| s.id == args.id) {
        return Err(CliError::user(
            CMD_ADD,
            format!(
                "duplicate scope id `{}`; use `scope edit` to change it",
                args.id
            ),
        ));
    }

    let scope = ScopeYaml {
        id: args.id.clone(),
        name: args.name.clone(),
        applies_to: args.applies_to.clone(),
        aliases: args.aliases.clone(),
        codeowners: args.codeowners.clone(),
    };
    upsert_scope(&mut file, scope)
        .map_err(|e| CliError::internal(CMD_ADD, format!("upsert scope: {e}")))?;
    save_file(&ctx.ws.root, &file).map_err(|e| CliError::user(CMD_ADD, e.to_string()))?;

    Ok(CommandOutcome::ScopeWrite(ScopeWriteOutcome::new(
        CMD_ADD,
        "added",
        vec![args.id.clone()],
        &file,
    )))
}

/// `firetrail scope edit` — apply only the provided changes in place.
pub fn edit(args: &ScopeEditArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_EDIT, global.workspace.as_deref())?;
    let mut file = load_file(&ctx.ws.root)
        .map_err(|e| CliError::internal(CMD_EDIT, format!("load scopes: {e}")))?;

    // Find the existing scope, clone it, and apply the requested changes.
    let mut scope = file
        .scopes
        .iter()
        .find(|s| s.id == args.id)
        .cloned()
        .ok_or_else(|| CliError::NotFound {
            command: CMD_EDIT.into(),
            what: args.id.clone(),
        })?;

    if args.clear_name {
        scope.name = None;
    } else if args.name.is_some() {
        scope.name.clone_from(&args.name);
    }
    if args.clear_codeowners {
        scope.codeowners = None;
    } else if args.codeowners.is_some() {
        scope.codeowners.clone_from(&args.codeowners);
    }
    // Vec fields replace the stored list only when at least one value is given.
    if !args.applies_to.is_empty() {
        scope.applies_to.clone_from(&args.applies_to);
    }
    if !args.aliases.is_empty() {
        scope.aliases.clone_from(&args.aliases);
    }

    upsert_scope(&mut file, scope)
        .map_err(|e| CliError::internal(CMD_EDIT, format!("upsert scope: {e}")))?;
    save_file(&ctx.ws.root, &file).map_err(|e| CliError::user(CMD_EDIT, e.to_string()))?;

    Ok(CommandOutcome::ScopeWrite(ScopeWriteOutcome::new(
        CMD_EDIT,
        "updated",
        vec![args.id.clone()],
        &file,
    )))
}

/// `firetrail scope rm` — remove a scope by id (errors if absent).
pub fn rm(args: &ScopeRmArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_RM, global.workspace.as_deref())?;
    let mut file = load_file(&ctx.ws.root)
        .map_err(|e| CliError::internal(CMD_RM, format!("load scopes: {e}")))?;

    remove_scope(&mut file, &args.id).map_err(|e| CliError::user(CMD_RM, e.to_string()))?;
    save_file(&ctx.ws.root, &file).map_err(|e| CliError::user(CMD_RM, e.to_string()))?;

    Ok(CommandOutcome::ScopeWrite(ScopeWriteOutcome::new(
        CMD_RM,
        "removed",
        vec![args.id.clone()],
        &file,
    )))
}

/// `firetrail scope reorder` — reorder scopes to the given full id list.
pub fn reorder_cmd(
    args: &ScopeReorderArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(CMD_REORDER, global.workspace.as_deref())?;
    let mut file = load_file(&ctx.ws.root)
        .map_err(|e| CliError::internal(CMD_REORDER, format!("load scopes: {e}")))?;

    reorder(&mut file, &args.ids).map_err(|e| CliError::user(CMD_REORDER, e.to_string()))?;
    save_file(&ctx.ws.root, &file).map_err(|e| CliError::user(CMD_REORDER, e.to_string()))?;

    Ok(CommandOutcome::ScopeWrite(ScopeWriteOutcome::new(
        CMD_REORDER,
        "reordered",
        args.ids.clone(),
        &file,
    )))
}
