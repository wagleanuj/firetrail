//! Transport-agnostic scope-registry ops (Wave 3-A reads + scope-authoring writes).
//!
//! Read views over `.firetrail/scopes.yaml` (loaded via [`ft_scope::load`]) plus
//! the write path ([`add`] / [`edit`] / [`remove`] / [`reorder`]) which wraps the
//! [`ft_scope::writer`] API, and [`preview`] (a read that surfaces per-scope
//! match counts + coverage warnings for the live editor). Mirrors
//! `ft_cli::commands::scope` but conforms to the ops boundary contract: no
//! `println!`, no clap, no axum, no stdin. Each op takes
//! `(&Workspace, &Identity, Input, &EventBus)` and returns
//! `Result<Output, OpsError>`. Every write emits a [`Event::ScopeUpdated`].
//!
//! Every input carries an optional `request_id` for SSE coalescing in the GUI
//! (kept symmetric with the rest of the crate so transports do not need to
//! special-case scope reads).
//!
//! ft-cli's existing `scope` subcommands are NOT rewired here; that is tracked
//! under firetrail-xy6.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ft_scope::writer::{
    load_file, remove_scope, reorder as writer_reorder, save_file, upsert_scope,
};
use ft_scope::{Scope, ScopeError, ScopeRegistry, ScopeYaml, load};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

// ─────────────────────────────────────────────────────────────────────────────
// Output shapes
// ─────────────────────────────────────────────────────────────────────────────

/// Summary view of a single scope (used by [`list`] and [`show`]).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// A single CODEOWNERS line.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeOwnersRow {
    /// Raw glob pattern.
    pub pattern: String,
    /// Owners (identity strings as they appear in the CODEOWNERS file).
    pub owners: Vec<String>,
}

/// Detail view of a single scope.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDetail {
    /// Summary fields.
    pub summary: ScopeSummary,
    /// Parsed CODEOWNERS entries.
    pub codeowners: Vec<CodeOwnersRow>,
}

/// One alias → scope-id entry.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasEntry {
    /// Alias text.
    pub alias: String,
    /// Scope id the alias resolves to.
    pub scope_id: String,
}

/// Output of [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeListOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOutput {
    /// Every scope, in load order.
    pub scopes: Vec<ScopeSummary>,
}

/// Output of [`show`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeShowOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowOutput {
    /// The scope's full detail.
    pub scope: ScopeDetail,
}

/// Output of [`aliases`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeAliasesOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasesOutput {
    /// Alphabetical alias → scope-id entries.
    pub aliases: Vec<AliasEntry>,
}

/// Output of [`owners`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeOwnersOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnersOutput {
    /// Resolved path (string form).
    pub path: String,
    /// Identity strings, in CODEOWNERS order.
    pub owners: Vec<String>,
}

/// One scope as the editor renders it after a write (declaration order).
///
/// The raw [`ScopeYaml`] mirror (so the editor can round-trip a write without
/// re-deriving `name`/`aliases` from the compiled registry).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeYamlView {
    /// Canonical id.
    pub id: String,
    /// Display name (if set).
    pub name: Option<String>,
    /// `applies_to` glob patterns.
    pub applies_to: Vec<String>,
    /// Declared aliases.
    pub aliases: Vec<String>,
    /// CODEOWNERS path (string form, if set).
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

/// Output of every scope *write* op (`add` / `edit` / `remove` / `reorder`).
///
/// Carries the full, post-write scopes list in declaration order so the editor
/// can re-render without a follow-up `GET /api/scope`.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeWriteOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutput {
    /// The full scopes list after the write, in declaration order.
    pub scopes: Vec<ScopeYamlView>,
}

/// One per-scope preview row: how many tracked files the scope matches.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeMatchRow {
    /// Canonical scope id.
    pub id: String,
    /// Tracked files (at `HEAD`) this scope's globs match.
    pub match_count: usize,
}

/// Output of [`preview`] — per-scope match counts plus advisory warnings.
///
/// Warnings mirror the doctor's per-scope checks: a scope matching zero tracked
/// files, and a later-declared broad scope shadowing an earlier narrower one.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopePreviewView"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopePreviewView {
    /// Per-scope match counts, in declaration order.
    pub scopes: Vec<ScopeMatchRow>,
    /// Advisory warnings (zero-match globs, broad-last shadowing).
    pub warnings: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Inputs
// ─────────────────────────────────────────────────────────────────────────────

/// Input for [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeListInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInput {
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`show`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeShowInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowInput {
    /// Scope id or alias.
    pub id: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`aliases`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeAliasesInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasesInput {
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`owners`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "ScopeOwnersInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnersInput {
    /// Repo-relative or absolute path to look up.
    pub path: PathBuf,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`add`] — a complete new scope (becomes last-declared).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeInput {
    /// Canonical scope id (unique).
    pub id: String,
    /// Optional display name.
    #[serde(default)]
    pub name: Option<String>,
    /// `applies_to` glob patterns (at least one required).
    #[serde(default)]
    pub applies_to: Vec<String>,
    /// Declared aliases.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Optional CODEOWNERS path (repo-relative).
    #[serde(default)]
    pub codeowners: Option<PathBuf>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`edit`] — partial update of an existing scope, by id.
///
/// Every field is `Option`: `None` leaves the stored value untouched. The
/// nullable fields (`name`, `codeowners`) use `Option<Option<T>>` so the editor
/// can distinguish "leave alone" (`None`) from "clear" (`Some(None)`). Vec
/// fields replace the stored list when `Some` (an empty `Some(vec![])` clears
/// it — validation then rejects an empty `applies_to`).
#[allow(clippy::option_option)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeEditInput {
    /// New display name (`Some(None)` clears it).
    #[serde(default)]
    pub name: Option<Option<String>>,
    /// Replacement `applies_to` glob list.
    #[serde(default)]
    pub applies_to: Option<Vec<String>>,
    /// Replacement alias list.
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    /// New CODEOWNERS path (`Some(None)` clears it).
    #[serde(default)]
    pub codeowners: Option<Option<PathBuf>>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Ops
// ─────────────────────────────────────────────────────────────────────────────

fn open_registry(ws: &Workspace) -> Result<ScopeRegistry, OpsError> {
    load(&ws.root).map_err(|e| OpsError::Internal(anyhow::anyhow!("load scopes: {e}")))
}

fn summary_of(s: &Scope) -> ScopeSummary {
    ScopeSummary {
        id: s.id.clone(),
        name: s.name.clone(),
        applies_to: s.applies_to_patterns.clone(),
        aliases: s.aliases.clone(),
        has_codeowners: s.codeowners.is_some(),
    }
}

fn detail_of(s: &Scope) -> ScopeDetail {
    let codeowners = s
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
    ScopeDetail {
        summary: summary_of(s),
        codeowners,
    }
}

/// `scope list` op.
#[allow(clippy::needless_pass_by_value)]
pub fn list(
    ws: &Workspace,
    _identity: &Identity,
    _input: ListInput,
    _events: &EventBus,
) -> Result<ListOutput, OpsError> {
    let registry = open_registry(ws)?;
    let scopes = registry.scopes().iter().map(summary_of).collect();
    Ok(ListOutput { scopes })
}

/// `scope show` op.
#[allow(clippy::needless_pass_by_value)]
pub fn show(
    ws: &Workspace,
    _identity: &Identity,
    input: ShowInput,
    _events: &EventBus,
) -> Result<ShowOutput, OpsError> {
    let registry = open_registry(ws)?;
    let scope = registry
        .get(&input.id)
        .or_else(|| registry.resolve_alias(&input.id))
        .ok_or_else(|| OpsError::not_found("scope", input.id.clone()))?;
    Ok(ShowOutput {
        scope: detail_of(scope),
    })
}

/// `scope aliases` op.
#[allow(clippy::needless_pass_by_value)]
pub fn aliases(
    ws: &Workspace,
    _identity: &Identity,
    _input: AliasesInput,
    _events: &EventBus,
) -> Result<AliasesOutput, OpsError> {
    let registry = open_registry(ws)?;
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
    Ok(AliasesOutput { aliases: entries })
}

/// `scope owners` op.
#[allow(clippy::needless_pass_by_value)]
pub fn owners(
    ws: &Workspace,
    _identity: &Identity,
    input: OwnersInput,
    _events: &EventBus,
) -> Result<OwnersOutput, OpsError> {
    let registry = open_registry(ws)?;
    let resolved = registry.owners_for_path(&input.path);
    let owners: Vec<String> = resolved.iter().map(|i| i.as_str().to_string()).collect();
    Ok(OwnersOutput {
        path: input.path.display().to_string(),
        owners,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Write ops
// ─────────────────────────────────────────────────────────────────────────────

/// Map a writer [`ScopeError`] onto the transport-agnostic [`OpsError`].
///
/// Validation-class failures (bad glob, empty `applies_to`, duplicate alias,
/// reorder mismatch) become [`OpsError::Validation`] so the HTTP adapter
/// returns 4xx; a missing scope becomes [`OpsError::NotFound`]; anything else
/// (IO / YAML) is internal.
fn map_scope_error(err: ScopeError) -> OpsError {
    match err {
        ScopeError::ScopeNotFound { id } => OpsError::not_found("scope", id),
        ScopeError::InvalidGlob { .. } | ScopeError::EmptyAppliesTo { .. } => {
            OpsError::validation("appliesTo", format!("{err}"))
        }
        ScopeError::DuplicateAlias { .. } => OpsError::validation("aliases", format!("{err}")),
        ScopeError::DuplicateScopeId { .. } => OpsError::validation("id", format!("{err}")),
        ScopeError::ReorderMismatch => OpsError::validation("orderedIds", format!("{err}")),
        ScopeError::Io { .. }
        | ScopeError::Yaml { .. }
        | ScopeError::CodeOwners { .. }
        | ScopeError::InvalidCodeOwnersGlob { .. } => {
            OpsError::Internal(anyhow::anyhow!("scopes.yaml: {err}"))
        }
    }
}

/// Build the [`WriteOutput`] from the in-memory file model.
fn write_output(file: &ft_scope::ScopesFile) -> WriteOutput {
    WriteOutput {
        scopes: file.scopes.iter().map(ScopeYamlView::from).collect(),
    }
}

/// Emit a [`Event::ScopeUpdated`] for `scope`, tagged with `request_id` if set.
fn emit_scope_updated(events: &EventBus, scope: &str, request_id: Option<&str>) {
    let event = Event::ScopeUpdated {
        scope: scope.to_string(),
    };
    match request_id {
        Some(rid) => events.emit_with_request(rid.to_string(), event),
        None => events.emit(event),
    }
}

/// `scope add` op — append a new scope (becomes last-declared).
///
/// Refuses to clobber an existing id (mirrors the CLI's explicit duplicate
/// guard); use [`edit`] to change a scope in place.
#[allow(clippy::needless_pass_by_value)]
pub fn add(
    ws: &Workspace,
    _identity: &Identity,
    input: ScopeInput,
    events: &EventBus,
) -> Result<WriteOutput, OpsError> {
    let mut file = load_file(&ws.root).map_err(map_scope_error)?;

    if file.scopes.iter().any(|s| s.id == input.id) {
        return Err(OpsError::conflict(format!(
            "duplicate scope id `{}`; use edit to change it",
            input.id
        )));
    }

    let scope = ScopeYaml {
        id: input.id.clone(),
        name: input.name.clone(),
        applies_to: input.applies_to.clone(),
        aliases: input.aliases.clone(),
        codeowners: input.codeowners.clone(),
    };
    upsert_scope(&mut file, scope).map_err(map_scope_error)?;
    save_file(&ws.root, &file).map_err(map_scope_error)?;

    emit_scope_updated(events, &input.id, input.request_id.as_deref());
    Ok(write_output(&file))
}

/// `scope edit` op — apply only the provided changes to an existing scope.
///
/// Errors with [`OpsError::NotFound`] when `id` is absent.
#[allow(clippy::needless_pass_by_value)]
pub fn edit(
    ws: &Workspace,
    _identity: &Identity,
    id: &str,
    input: ScopeEditInput,
    events: &EventBus,
) -> Result<WriteOutput, OpsError> {
    let mut file = load_file(&ws.root).map_err(map_scope_error)?;

    let mut scope = file
        .scopes
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| OpsError::not_found("scope", id))?;

    if let Some(name) = input.name {
        scope.name = name;
    }
    if let Some(applies_to) = input.applies_to {
        scope.applies_to = applies_to;
    }
    if let Some(aliases) = input.aliases {
        scope.aliases = aliases;
    }
    if let Some(codeowners) = input.codeowners {
        scope.codeowners = codeowners;
    }

    upsert_scope(&mut file, scope).map_err(map_scope_error)?;
    save_file(&ws.root, &file).map_err(map_scope_error)?;

    emit_scope_updated(events, id, input.request_id.as_deref());
    Ok(write_output(&file))
}

/// `scope remove` op — delete a scope by id (errors if absent).
#[allow(clippy::needless_pass_by_value)]
pub fn remove(
    ws: &Workspace,
    _identity: &Identity,
    id: &str,
    events: &EventBus,
) -> Result<WriteOutput, OpsError> {
    let mut file = load_file(&ws.root).map_err(map_scope_error)?;
    remove_scope(&mut file, id).map_err(map_scope_error)?;
    save_file(&ws.root, &file).map_err(map_scope_error)?;

    emit_scope_updated(events, id, None);
    Ok(write_output(&file))
}

/// `scope reorder` op — reorder scopes to match `ordered_ids` (a permutation of
/// the existing ids). Declaration order is semantic (last-declared-wins).
#[allow(clippy::needless_pass_by_value)]
pub fn reorder(
    ws: &Workspace,
    _identity: &Identity,
    ordered_ids: &[String],
    events: &EventBus,
) -> Result<WriteOutput, OpsError> {
    let mut file = load_file(&ws.root).map_err(map_scope_error)?;
    writer_reorder(&mut file, ordered_ids).map_err(map_scope_error)?;
    save_file(&ws.root, &file).map_err(map_scope_error)?;

    // No single affected id — emit a registry-wide change.
    emit_scope_updated(events, "*", None);
    Ok(write_output(&file))
}

/// `scope preview` op (read; no event).
///
/// For the **currently saved** scopes, count how many tracked files (at `HEAD`)
/// each scope's globs match, and surface the same advisory warnings the doctor's
/// `check_scope_glob_coverage` produces: a scope matching zero tracked files,
/// and a later-declared broad scope shadowing an earlier narrower one. When git
/// can't be read (no repo / no commit) match counts are all zero and the
/// glob/shadow warnings are skipped (they require a populated tree).
#[allow(clippy::needless_pass_by_value)]
pub fn preview(
    ws: &Workspace,
    _identity: &Identity,
    _events: &EventBus,
) -> Result<ScopePreviewView, OpsError> {
    let registry = open_registry(ws)?;

    // Tracked files at HEAD; `None` when the repo / HEAD can't be read.
    let files: Option<Vec<PathBuf>> = ft_git::Repo::open(&ws.root)
        .and_then(|repo| repo.list_files_at_ref("HEAD", "**"))
        .ok();

    // Per-scope matched-file index sets, in declaration order. Without a tree,
    // every scope matches nothing.
    let matched: Vec<(String, Vec<usize>)> = registry
        .scopes()
        .iter()
        .map(|scope| {
            let hits: Vec<usize> = files
                .as_ref()
                .map(|fs| {
                    fs.iter()
                        .enumerate()
                        .filter(|(_, p)| scope.matches_path(p))
                        .map(|(i, _)| i)
                        .collect()
                })
                .unwrap_or_default();
            (scope.id.clone(), hits)
        })
        .collect();

    let scopes: Vec<ScopeMatchRow> = matched
        .iter()
        .map(|(id, hits)| ScopeMatchRow {
            id: id.clone(),
            match_count: hits.len(),
        })
        .collect();

    let mut warnings: Vec<String> = Vec::new();

    // The glob/shadow warnings are advisory and require a populated tree.
    if files.is_some() {
        let empty: Vec<&str> = matched
            .iter()
            .filter(|(_, hits)| hits.is_empty())
            .map(|(id, _)| id.as_str())
            .collect();
        if !empty.is_empty() {
            warnings.push(format!(
                "{} scope(s) match zero tracked files: {}",
                empty.len(),
                empty.join(", ")
            ));
        }

        // Shadowing: a broad scope B declared AFTER a narrower scope A, where
        // A's matched files are a strict subset of B's. Last-declared-wins means
        // B always wins everywhere A would have, so A never governs a file. Only
        // emit when A is non-empty (an empty A is the zero-match case).
        for (a_idx, (a_id, a_hits)) in matched.iter().enumerate() {
            if a_hits.is_empty() {
                continue;
            }
            let a_set: BTreeSet<usize> = a_hits.iter().copied().collect();
            for (b_id, b_hits) in matched.iter().skip(a_idx + 1) {
                let b_set: BTreeSet<usize> = b_hits.iter().copied().collect();
                if a_set.is_subset(&b_set) && b_set.len() > a_set.len() {
                    warnings.push(format!("`{b_id}` shadows `{a_id}`"));
                }
            }
        }
    }

    Ok(ScopePreviewView { scopes, warnings })
}
