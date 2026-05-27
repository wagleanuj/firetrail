//! # ft-scope
//!
//! Multi-scope routing logic and CODEOWNERS-style scope resolution.
//!
//! This crate is responsible for two related concerns:
//!
//! 1. Loading `.firetrail/scopes.yaml` into a [`ScopeRegistry`] that can
//!    answer "which scope(s) does this path belong to?" and "what aliases
//!    resolve to this scope?".
//! 2. Resolving CODEOWNERS rules into the [`Identity`](ft_core::Identity)
//!    values that should be routed for review when a record touches a given
//!    path.
//!
//! It also surfaces cross-scope decision conflicts via
//! [`detect_conflicting_decisions`] — see [`conflict`] for details.
//!
//! ## Relevant ADRs
//!
//! - ADR-0004 — Multi-scope records (`owning_scope`, `affected_scopes`,
//!   `applies_to`)
//! - ADR-0008 — Identity registry (the target type for CODEOWNERS owners)

pub mod codeowners;
pub mod conflict;
pub mod error;
pub mod registry;

pub use codeowners::CodeOwnersEntry;
pub use conflict::{ConflictingDecision, DecisionOccurrence, detect_conflicting_decisions};
pub use error::ScopeError;
pub use registry::{SCOPES_FILE, Scope, ScopeRegistry};

use std::path::Path;

/// Convenience: load a [`ScopeRegistry`] from `<workspace_root>/.firetrail/scopes.yaml`.
///
/// Equivalent to [`ScopeRegistry::load`].
///
/// # Errors
///
/// See [`ScopeRegistry::load`].
pub fn load(workspace_root: &Path) -> Result<ScopeRegistry, ScopeError> {
    ScopeRegistry::load(workspace_root)
}
