//! Transport-agnostic scope-registry ops (Wave 3-A).
//!
//! Read-only views over `.firetrail/scopes.yaml` (loaded via [`ft_scope::load`]).
//! Mirrors `ft_cli::commands::scope` but conforms to the ops boundary contract:
//! no `println!`, no clap, no axum, no stdin. Each op takes
//! `(&Workspace, &Identity, Input, &EventBus)` and returns
//! `Result<Output, OpsError>`.
//!
//! Every input carries an optional `request_id` for SSE coalescing in the GUI,
//! even though the current ops are read-only (kept symmetric with the rest of
//! the crate so transports do not need to special-case scope reads).
//!
//! ft-cli's existing `scope` subcommands are NOT rewired here; that is tracked
//! under firetrail-xy6.

use std::path::PathBuf;

use ft_scope::{Scope, ScopeRegistry, load};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
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
