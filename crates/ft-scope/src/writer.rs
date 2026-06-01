//! Writer for `.firetrail/scopes.yaml`.
//!
//! Where [`crate::registry`] only *reads* the scopes file into a compiled
//! [`ScopeRegistry`](crate::ScopeRegistry), this module provides the *write*
//! path: an order-stable, regenerate-the-block writer that operates on the raw
//! [`ScopesFile`] / [`ScopeYaml`] model.
//!
//! ## Design
//!
//! - **Order is semantic.** Scope resolution is last-declared-wins, so the
//!   writer preserves declaration order on every operation. New scopes are
//!   appended (they become last-declared); upserts replace in place.
//! - **Regenerate the block.** On save the whole file is re-serialized
//!   deterministically and a tool-managed [`HEADER`] comment is prepended.
//!   Hand-written comments are **not** preserved (acceptable for v1).
//! - **Validate before write.** [`save_file`] runs [`validate`] first, so an
//!   invalid model never reaches disk.

use std::fs;
use std::path::Path;

use globset::Glob;

use crate::error::ScopeError;
use crate::registry::{SCOPES_FILE, ScopeYaml, ScopesFile};

/// Tool-managed header prepended to every written scopes file.
pub const HEADER: &str =
    "# Managed by `firetrail scope`. Order matters: resolution is last-declared-wins.";

/// Load the raw scopes model from `<root>/.firetrail/scopes.yaml`.
///
/// A missing file is **not** an error — it yields an empty [`ScopesFile`].
/// This mirrors [`ScopeRegistry::load`](crate::ScopeRegistry::load) but returns
/// the raw, mutable model instead of a compiled registry.
///
/// # Errors
///
/// - [`ScopeError::Io`] if the file exists but cannot be read.
/// - [`ScopeError::Yaml`] if the file does not parse.
pub fn load_file(root: &Path) -> Result<ScopesFile, ScopeError> {
    let path = root.join(SCOPES_FILE);
    if !path.exists() {
        return Ok(ScopesFile::default());
    }
    let text = fs::read_to_string(&path).map_err(|source| ScopeError::Io {
        path: path.clone(),
        source,
    })?;
    serde_yaml::from_str(&text).map_err(|source| ScopeError::Yaml { path, source })
}

/// Validate then write `file` to `<root>/.firetrail/scopes.yaml`.
///
/// The `.firetrail/` directory is created if needed. The written file is the
/// [`HEADER`] comment followed by the deterministically serialized model, with
/// declaration order preserved.
///
/// # Errors
///
/// - Any [`validate`] error (the model is checked before writing).
/// - [`ScopeError::Yaml`] if serialization fails.
/// - [`ScopeError::Io`] if the directory or file cannot be written.
pub fn save_file(root: &Path, file: &ScopesFile) -> Result<(), ScopeError> {
    validate(file)?;

    let dir = root.join(".firetrail");
    let path = dir.join("scopes.yaml");

    fs::create_dir_all(&dir).map_err(|source| ScopeError::Io {
        path: dir.clone(),
        source,
    })?;

    let body = serde_yaml::to_string(file).map_err(|source| ScopeError::Yaml {
        path: path.clone(),
        source,
    })?;
    let contents = format!("{HEADER}\n{body}");

    fs::write(&path, contents).map_err(|source| ScopeError::Io { path, source })
}

/// Insert or replace `scope` by id.
///
/// If a scope with the same id already exists it is replaced **in place**
/// (position preserved). Otherwise `scope` is **appended**, making it the
/// last-declared scope.
///
/// # Errors
///
/// This function never fails today, but returns `Result` for forward
/// compatibility and to compose with the other write operations.
#[allow(
    clippy::unnecessary_wraps,
    reason = "Result kept for API symmetry with the other write ops and future validation."
)]
pub fn upsert_scope(file: &mut ScopesFile, scope: ScopeYaml) -> Result<(), ScopeError> {
    if let Some(existing) = file.scopes.iter_mut().find(|s| s.id == scope.id) {
        *existing = scope;
    } else {
        file.scopes.push(scope);
    }
    Ok(())
}

/// Remove the scope with the given id.
///
/// # Errors
///
/// - [`ScopeError::ScopeNotFound`] if no scope has that id.
pub fn remove_scope(file: &mut ScopesFile, id: &str) -> Result<(), ScopeError> {
    let before = file.scopes.len();
    file.scopes.retain(|s| s.id != id);
    if file.scopes.len() == before {
        return Err(ScopeError::ScopeNotFound { id: id.to_string() });
    }
    Ok(())
}

/// Reorder scopes to match `ordered_ids`.
///
/// `ordered_ids` must be a permutation of the existing scope ids (same set, no
/// duplicates, no missing/extra ids).
///
/// # Errors
///
/// - [`ScopeError::ReorderMismatch`] if `ordered_ids` is not a permutation of
///   the existing ids.
pub fn reorder(file: &mut ScopesFile, ordered_ids: &[String]) -> Result<(), ScopeError> {
    if ordered_ids.len() != file.scopes.len() {
        return Err(ScopeError::ReorderMismatch);
    }

    let mut reordered: Vec<ScopeYaml> = Vec::with_capacity(file.scopes.len());
    let mut remaining: Vec<ScopeYaml> = std::mem::take(&mut file.scopes);

    for id in ordered_ids {
        let Some(pos) = remaining.iter().position(|s| &s.id == id) else {
            // Either an unknown id or a duplicate (already consumed). Restore
            // and report a mismatch.
            file.scopes = reordered;
            file.scopes.append(&mut remaining);
            return Err(ScopeError::ReorderMismatch);
        };
        reordered.push(remaining.remove(pos));
    }

    // `remaining` must be empty: lengths matched and every id was consumed.
    debug_assert!(remaining.is_empty());
    file.scopes = reordered;
    Ok(())
}

/// Validate the model before it is written.
///
/// Checks that:
/// - every `applies_to` glob compiles,
/// - scope ids are unique,
/// - aliases are unique across all scopes,
/// - every scope declares at least one `applies_to` pattern.
///
/// # Errors
///
/// - [`ScopeError::InvalidGlob`] for an `applies_to` pattern that fails to
///   compile.
/// - [`ScopeError::DuplicateScopeId`] for a repeated id.
/// - [`ScopeError::DuplicateAlias`] for an alias claimed by two scopes.
/// - [`ScopeError::EmptyAppliesTo`] for a scope with no patterns.
pub fn validate(file: &ScopesFile) -> Result<(), ScopeError> {
    let mut seen_ids: Vec<&str> = Vec::with_capacity(file.scopes.len());
    // alias -> first scope id that claimed it
    let mut seen_aliases: Vec<(&str, &str)> = Vec::new();

    for scope in &file.scopes {
        if seen_ids.contains(&scope.id.as_str()) {
            return Err(ScopeError::DuplicateScopeId {
                id: scope.id.clone(),
            });
        }
        seen_ids.push(&scope.id);

        if scope.applies_to.is_empty() {
            return Err(ScopeError::EmptyAppliesTo {
                id: scope.id.clone(),
            });
        }

        for pat in &scope.applies_to {
            Glob::new(pat).map_err(|source| ScopeError::InvalidGlob {
                scope_id: scope.id.clone(),
                pattern: pat.clone(),
                source,
            })?;
        }

        for alias in &scope.aliases {
            if let Some((_, first)) = seen_aliases.iter().find(|(a, _)| *a == alias.as_str()) {
                return Err(ScopeError::DuplicateAlias {
                    alias: alias.clone(),
                    first: (*first).to_string(),
                    second: scope.id.clone(),
                });
            }
            seen_aliases.push((alias, &scope.id));
        }
    }

    Ok(())
}
