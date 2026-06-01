//! [`ScopeRegistry`] — loads and queries `.firetrail/scopes.yaml`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

use ft_core::Identity;

use crate::codeowners::{self, CodeOwnersEntry};
use crate::error::ScopeError;

/// Location of the scopes file inside the workspace root.
pub const SCOPES_FILE: &str = ".firetrail/scopes.yaml";

/// On-disk YAML shape for a single scope. Parsed and then lifted into
/// [`Scope`] (which carries compiled matchers).
///
/// This is the *raw* serializable model used by both the loader
/// ([`ScopeRegistry::load`]) and the writer ([`crate::writer`]). The serde
/// renames are shared by serialization and deserialization so the file
/// round-trips through the canonical field names (`applies_to`,
/// `enabled_scopes`). `skip_serializing_if` keeps the emitted file clean for
/// empty/absent optional fields; it does **not** affect deserialization.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ScopeYaml {
    /// Canonical scope id (e.g. `apps/checkout`).
    pub id: String,
    /// Optional display name; defaults to the id when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Glob patterns this scope applies to, in declaration order.
    #[serde(
        default,
        rename = "applies_to",
        alias = "appliesTo",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub applies_to: Vec<String>,
    /// Aliases that resolve to this scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Optional path to a CODEOWNERS file for this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codeowners: Option<PathBuf>,
}

/// On-disk YAML shape for the top-level scopes file.
///
/// Declaration order of [`Self::scopes`] is **semantic**: scope resolution is
/// last-declared-wins, so writers must preserve order.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ScopesFile {
    /// All declared scopes, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<ScopeYaml>,
    /// Optional pilot-rollout filter. `None` means every scope is enabled.
    #[serde(
        default,
        rename = "enabled_scopes",
        alias = "enabledScopes",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled_scopes: Option<Vec<String>>,
}

/// A single resolved scope. Globs are compiled at load time.
#[derive(Debug, Clone)]
pub struct Scope {
    /// Canonical scope id (e.g. `apps/checkout`).
    pub id: String,
    /// Display name. Defaults to [`Self::id`] when omitted.
    pub name: String,
    /// Original `applies_to` patterns (for diagnostics / round-tripping).
    pub applies_to_patterns: Vec<String>,
    /// Compiled matchers for [`Self::applies_to_patterns`], in source order.
    pub applies_to: Vec<GlobMatcher>,
    /// Aliases that resolve to this scope.
    pub aliases: Vec<String>,
    /// Parsed CODEOWNERS entries, if a codeowners file was declared.
    pub codeowners: Option<Vec<CodeOwnersEntry>>,
}

impl Scope {
    /// Returns true if any of this scope's `applies_to` matchers matches
    /// `path`.
    #[must_use]
    pub fn matches_path(&self, path: &Path) -> bool {
        self.applies_to.iter().any(|m| m.is_match(path))
    }
}

/// The loaded scopes registry.
#[derive(Debug, Clone, Default)]
pub struct ScopeRegistry {
    scopes: Vec<Scope>,
    /// alias → index into `scopes`
    alias_index: HashMap<String, usize>,
    /// id → index into `scopes`
    id_index: HashMap<String, usize>,
    /// Pilot rollout filter. `None` means all scopes are enabled.
    enabled_scopes: Option<Vec<String>>,
}

impl ScopeRegistry {
    /// Load the registry from `<workspace_root>/.firetrail/scopes.yaml`.
    ///
    /// A missing scopes file is **not** an error — it produces an empty
    /// registry. Callers that require explicit configuration should check
    /// [`Self::is_empty`].
    ///
    /// # Errors
    ///
    /// - [`ScopeError::Io`] if the file exists but cannot be read.
    /// - [`ScopeError::Yaml`] if the file does not parse.
    /// - [`ScopeError::InvalidGlob`] if any `applies_to` pattern is invalid.
    /// - [`ScopeError::CodeOwners`] / [`ScopeError::InvalidCodeOwnersGlob`]
    ///   when a referenced CODEOWNERS file fails to load.
    /// - [`ScopeError::DuplicateScopeId`] / [`ScopeError::DuplicateAlias`].
    pub fn load(workspace_root: &Path) -> Result<Self, ScopeError> {
        let path = workspace_root.join(SCOPES_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(&path).map_err(|source| ScopeError::Io {
            path: path.clone(),
            source,
        })?;
        let parsed: ScopesFile =
            serde_yaml::from_str(&text).map_err(|source| ScopeError::Yaml {
                path: path.clone(),
                source,
            })?;

        let mut scopes: Vec<Scope> = Vec::with_capacity(parsed.scopes.len());
        let mut id_index: HashMap<String, usize> = HashMap::new();
        let mut alias_index: HashMap<String, usize> = HashMap::new();

        for scope_yaml in parsed.scopes {
            let ScopeYaml {
                id,
                name,
                applies_to,
                aliases,
                codeowners: codeowners_path,
            } = scope_yaml;

            if id_index.contains_key(&id) {
                return Err(ScopeError::DuplicateScopeId { id });
            }

            let mut matchers = Vec::with_capacity(applies_to.len());
            for pat in &applies_to {
                let glob = Glob::new(pat).map_err(|source| ScopeError::InvalidGlob {
                    scope_id: id.clone(),
                    pattern: pat.clone(),
                    source,
                })?;
                matchers.push(glob.compile_matcher());
            }

            let codeowners = if let Some(rel) = codeowners_path {
                let full = workspace_root.join(&rel);
                let text = fs::read_to_string(&full).map_err(|source| ScopeError::CodeOwners {
                    scope_id: id.clone(),
                    path: full.clone(),
                    source,
                })?;
                Some(codeowners::parse(&full, &text)?)
            } else {
                None
            };

            let idx = scopes.len();
            let display_name = name.unwrap_or_else(|| id.clone());

            // Aliases also include the scope id itself so `resolve_alias("apps/checkout")`
            // works even when no explicit alias is declared.
            let mut all_aliases = aliases.clone();
            if !all_aliases.iter().any(|a| a == &id) {
                all_aliases.push(id.clone());
            }
            for alias in &all_aliases {
                if let Some(prior_idx) = alias_index.get(alias) {
                    return Err(ScopeError::DuplicateAlias {
                        alias: alias.clone(),
                        first: scopes[*prior_idx].id.clone(),
                        second: id.clone(),
                    });
                }
                alias_index.insert(alias.clone(), idx);
            }
            id_index.insert(id.clone(), idx);

            scopes.push(Scope {
                id,
                name: display_name,
                applies_to_patterns: applies_to,
                applies_to: matchers,
                aliases,
                codeowners,
            });
        }

        Ok(Self {
            scopes,
            alias_index,
            id_index,
            enabled_scopes: parsed.enabled_scopes,
        })
    }

    /// Returns every loaded scope, in source order.
    #[must_use]
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// Returns true if no scopes were configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Returns every scope whose `applies_to` globs match `path` (case
    /// sensitive). Order matches the source-file order of [`Self::scopes`].
    #[must_use]
    pub fn scopes_for_path(&self, path: &Path) -> Vec<&Scope> {
        self.scopes
            .iter()
            .filter(|s| s.matches_path(path))
            .collect()
    }

    /// Resolve a scope by id or by any declared alias. Returns `None` if no
    /// scope matches.
    #[must_use]
    pub fn resolve_alias(&self, alias: &str) -> Option<&Scope> {
        self.alias_index.get(alias).map(|i| &self.scopes[*i])
    }

    /// Look up a scope by its canonical id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Scope> {
        self.id_index.get(id).map(|i| &self.scopes[*i])
    }

    /// Returns the explicit pilot-rollout list, if one was declared.
    ///
    /// `None` means every scope is enabled (the default). `Some(list)` is
    /// the literal `enabled_scopes` array from `scopes.yaml`, in source
    /// order; an empty list means *no* scope is enabled.
    #[must_use]
    pub fn enabled_scopes_list(&self) -> Option<&[String]> {
        self.enabled_scopes.as_deref()
    }

    /// Returns true if `scope_id` is enabled by the pilot-rollout filter.
    ///
    /// When `enabled_scopes` is omitted from the YAML, every scope is
    /// enabled. When present, only scopes whose id appears in the list are
    /// enabled (unknown ids return false).
    #[must_use]
    pub fn is_scope_enabled(&self, scope_id: &str) -> bool {
        match &self.enabled_scopes {
            None => true,
            Some(list) => list.iter().any(|s| s == scope_id),
        }
    }

    /// Resolve CODEOWNERS into [`Identity`] values for `path`.
    ///
    /// Walks every scope whose `applies_to` matches `path`. For each such
    /// scope, every CODEOWNERS entry whose pattern also matches contributes
    /// its owner strings. Owner strings are interpreted literally as identity
    /// names for M5 (per task spec), but those that fail [`Identity::new`]
    /// validation (e.g. contain whitespace) are silently skipped — the loader
    /// already validated the file, and tightening identity rules will surface
    /// in a future milestone.
    ///
    /// Returned identities are deduplicated while preserving first-seen
    /// order.
    #[must_use]
    pub fn owners_for_path(&self, path: &Path) -> Vec<Identity> {
        let mut seen: Vec<Identity> = Vec::new();
        for scope in self.scopes_for_path(path) {
            let Some(entries) = &scope.codeowners else {
                continue;
            };
            for entry in entries {
                if !entry.matcher.is_match(path) {
                    continue;
                }
                for owner in &entry.owners {
                    let Ok(id) = Identity::new(owner.clone()) else {
                        continue;
                    };
                    if !seen.iter().any(|existing| existing == &id) {
                        seen.push(id);
                    }
                }
            }
        }
        seen
    }
}
