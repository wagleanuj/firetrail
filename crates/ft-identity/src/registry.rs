//! Identity registry (ADR-0008, M5).
//!
//! The registry is the multi-identity layer that sits on top of the M1
//! resolver: where M1 answers "who is the current actor?", M5 answers
//! "what can that actor do, what other identities does it map to, and is
//! it still on staff?".
//!
//! The on-disk schema lives at `.firetrail/identities.yaml` and looks like:
//!
//! ```yaml
//! identities:
//!   - id: alice
//!     name: Alice Smith
//!     kind: human
//!     emails:
//!       - alice@example.com
//!       - alice.smith@personal.com
//!     machines: [laptop-1, laptop-2]
//!     capabilities:
//!       can_promote_verified: true
//!       can_close_high_risk: true
//!     status: active
//! ```
//!
//! Identities not declared in the file have no capabilities and cannot be
//! resolved by alias. The M1 resolver still produces an [`Identity`] for
//! them — strict mode (wired in a later milestone) is what causes a registry
//! miss to become a hard error.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::IdentityError;
use crate::capabilities::{CapabilityMatrix, IdentityKind, IdentityStatus};

// ---------------------------------------------------------------------------
// File names
// ---------------------------------------------------------------------------

/// Relative path of the registry file inside a workspace.
pub const REGISTRY_FILENAME: &str = ".firetrail/identities.yaml";

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// A single registered identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredIdentity {
    /// Canonical short id, e.g. `alice`, `bot-claude`.
    pub id: String,
    /// Human-readable display name.
    #[serde(default)]
    pub name: String,
    /// Identity kind. Drives capability defaults.
    pub kind: IdentityKind,
    /// All email aliases this identity answers to.
    #[serde(default)]
    pub emails: Vec<String>,
    /// Machine hostnames associated with this identity.
    #[serde(default)]
    pub machines: Vec<String>,
    /// Capability overrides. Missing fields fall back to kind defaults via
    /// [`RegisteredIdentity::effective_capabilities`].
    #[serde(default)]
    pub capabilities: PartialCapabilityMatrix,
    /// Lifecycle status. Defaults to [`IdentityStatus::Active`].
    #[serde(default)]
    pub status: IdentityStatus,
}

impl RegisteredIdentity {
    /// Compose the on-file capability overrides with the kind defaults to
    /// produce the effective capability matrix the policy layer should see.
    #[must_use]
    pub fn effective_capabilities(&self) -> CapabilityMatrix {
        let defaults = CapabilityMatrix::defaults_for_kind(self.kind);
        self.capabilities.apply_over(defaults)
    }
}

/// A capability matrix with every field optional. Used on the wire so absent
/// fields fall back to kind defaults rather than `false`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PartialCapabilityMatrix {
    /// Override for [`CapabilityMatrix::can_promote_verified`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_promote_verified: Option<bool>,
    /// Override for [`CapabilityMatrix::can_close_high_risk`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_close_high_risk: Option<bool>,
    /// Override for [`CapabilityMatrix::can_force_push`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_force_push: Option<bool>,
    /// Override for [`CapabilityMatrix::can_redact`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_redact: Option<bool>,
    /// Custom named capabilities. Merged into the effective matrix's
    /// [`CapabilityMatrix::extra`] map.
    #[serde(default, skip_serializing_if = "HashMap::is_empty", flatten)]
    pub extra: HashMap<String, bool>,
}

impl PartialCapabilityMatrix {
    /// Apply these overrides on top of a base matrix, returning the result.
    #[must_use]
    pub fn apply_over(&self, mut base: CapabilityMatrix) -> CapabilityMatrix {
        if let Some(v) = self.can_promote_verified {
            base.can_promote_verified = v;
        }
        if let Some(v) = self.can_close_high_risk {
            base.can_close_high_risk = v;
        }
        if let Some(v) = self.can_force_push {
            base.can_force_push = v;
        }
        if let Some(v) = self.can_redact {
            base.can_redact = v;
        }
        for (k, v) in &self.extra {
            base.extra.insert(k.clone(), *v);
        }
        base
    }
}

/// The wire shape of the registry file.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRegistry {
    /// All registered identities in declaration order.
    #[serde(default)]
    pub identities: Vec<RegisteredIdentity>,
}

impl IdentityRegistry {
    /// Build an empty registry.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Resolve a canonical [`RegisteredIdentity`] by id or any alias.
    ///
    /// The match is case-sensitive on the id, and case-insensitive on the
    /// email address local-part and domain (per RFC 5321 §2.4 the local-part
    /// is officially case-sensitive but in practice all major providers fold).
    #[must_use]
    pub fn resolve_canonical(&self, email_or_id: &str) -> Option<&RegisteredIdentity> {
        let needle_lower = email_or_id.to_ascii_lowercase();
        self.identities.iter().find(|ident| {
            ident.id == email_or_id
                || ident
                    .emails
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&needle_lower))
        })
    }

    /// Look up effective capabilities for a given id.
    #[must_use]
    pub fn capabilities(&self, id: &str) -> Option<CapabilityMatrix> {
        self.identities
            .iter()
            .find(|i| i.id == id)
            .map(RegisteredIdentity::effective_capabilities)
    }

    /// Convenience predicate: does `id` hold the named capability?
    ///
    /// Returns `false` when the id is unknown, when the identity is
    /// offboarded, or when the capability is set to `false`. Looks up
    /// well-known capability fields first, then falls through to the `extra`
    /// map.
    #[must_use]
    pub fn can(&self, id: &str, capability_name: &str) -> bool {
        let Some(ident) = self.identities.iter().find(|i| i.id == id) else {
            return false;
        };
        if !matches!(ident.status, IdentityStatus::Active) {
            return false;
        }
        let caps = ident.effective_capabilities();
        match capability_name {
            "can_promote_verified" => caps.can_promote_verified,
            "can_close_high_risk" => caps.can_close_high_risk,
            "can_force_push" => caps.can_force_push,
            "can_redact" => caps.can_redact,
            other => caps.extra.get(other).copied().unwrap_or(false),
        }
    }

    /// Mark an identity as offboarded. The caller is expected to follow up by
    /// running an offboarding sweep (see [`crate::sweep::find_live_claims_for`]).
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Unresolved`] when the id is not in the registry.
    pub fn offboard(&mut self, id: &str) -> Result<(), IdentityError> {
        let ident = self
            .identities
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| IdentityError::Unresolved(format!("unknown identity id `{id}`")))?;
        ident.status = IdentityStatus::Offboarded;
        Ok(())
    }

    /// Serialize the registry back to `.firetrail/identities.yaml`.
    ///
    /// Creates `.firetrail/` if it does not already exist. Overwrites any
    /// prior contents.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Io`] on any filesystem failure and
    /// [`IdentityError::Serialize`] when `serde_yaml` refuses to render the
    /// in-memory structure (e.g. a non-UTF-8 string slipped through).
    pub fn save(&self, workspace_root: &Path) -> Result<(), IdentityError> {
        let dir = workspace_root.join(".firetrail");
        std::fs::create_dir_all(&dir)?;
        let path = workspace_root.join(REGISTRY_FILENAME);
        let yaml =
            serde_yaml::to_string(self).map_err(|e| IdentityError::Serialize(e.to_string()))?;
        std::fs::write(path, yaml)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load `.firetrail/identities.yaml` from `workspace_root`.
///
/// Returns an empty registry when the file is missing — the registry is an
/// opt-in artifact and the M1 resolver path remains the source of truth for
/// unregistered workspaces.
///
/// # Errors
///
/// Returns [`IdentityError::Io`] for filesystem failures other than
/// `NotFound`, and [`IdentityError::Parse`] when the file exists but does not
/// deserialize as a [`IdentityRegistry`].
pub fn load_registry(workspace_root: &Path) -> Result<IdentityRegistry, IdentityError> {
    let path = workspace_root.join(REGISTRY_FILENAME);
    let contents = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(IdentityRegistry::empty()),
        Err(e) => return Err(IdentityError::Io(e)),
    };
    let registry: IdentityRegistry = serde_yaml::from_str(&contents)
        .map_err(|e| IdentityError::Parse(format!("{}: {e}", path.display())))?;
    Ok(registry)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample() -> IdentityRegistry {
        IdentityRegistry {
            identities: vec![
                RegisteredIdentity {
                    id: "alice".into(),
                    name: "Alice Smith".into(),
                    kind: IdentityKind::Human,
                    emails: vec![
                        "alice@example.com".into(),
                        "alice.smith@personal.com".into(),
                    ],
                    machines: vec!["laptop-1".into(), "laptop-2".into()],
                    capabilities: PartialCapabilityMatrix {
                        can_promote_verified: Some(true),
                        can_close_high_risk: Some(true),
                        ..Default::default()
                    },
                    status: IdentityStatus::Active,
                },
                RegisteredIdentity {
                    id: "bot-claude".into(),
                    name: "Claude".into(),
                    kind: IdentityKind::Bot,
                    emails: vec!["bot@example.com".into()],
                    machines: vec![],
                    capabilities: PartialCapabilityMatrix {
                        can_promote_verified: Some(false),
                        ..Default::default()
                    },
                    status: IdentityStatus::Active,
                },
            ],
        }
    }

    #[test]
    fn load_missing_file_returns_empty_registry() {
        let tmp = TempDir::new().unwrap();
        let r = load_registry(tmp.path()).unwrap();
        assert!(r.identities.is_empty());
    }

    #[test]
    fn round_trip_through_yaml() {
        let tmp = TempDir::new().unwrap();
        let original = sample();
        original.save(tmp.path()).unwrap();
        let reloaded = load_registry(tmp.path()).unwrap();
        assert_eq!(original, reloaded);
    }

    #[test]
    fn resolve_canonical_by_id() {
        let r = sample();
        let got = r.resolve_canonical("alice").unwrap();
        assert_eq!(got.id, "alice");
    }

    #[test]
    fn resolve_canonical_by_email() {
        let r = sample();
        let got = r.resolve_canonical("alice.smith@personal.com").unwrap();
        assert_eq!(got.id, "alice");
    }

    #[test]
    fn resolve_canonical_email_is_case_insensitive() {
        let r = sample();
        let got = r.resolve_canonical("ALICE@example.com").unwrap();
        assert_eq!(got.id, "alice");
    }

    #[test]
    fn resolve_canonical_returns_none_for_unknown() {
        let r = sample();
        assert!(r.resolve_canonical("ghost@example.com").is_none());
    }

    #[test]
    fn capabilities_reflect_overrides() {
        let r = sample();
        let alice = r.capabilities("alice").unwrap();
        assert!(alice.can_promote_verified);
        assert!(alice.can_close_high_risk);
        let bot = r.capabilities("bot-claude").unwrap();
        assert!(!bot.can_promote_verified);
        // bot default: can_close_high_risk = true (per spec)
        assert!(bot.can_close_high_risk);
    }

    #[test]
    fn can_predicate_handles_offboarded() {
        let mut r = sample();
        assert!(r.can("alice", "can_promote_verified"));
        r.offboard("alice").unwrap();
        assert!(!r.can("alice", "can_promote_verified"));
    }

    #[test]
    fn can_predicate_handles_unknown_id() {
        let r = sample();
        assert!(!r.can("ghost", "can_promote_verified"));
    }

    #[test]
    fn can_predicate_supports_extra_capabilities() {
        let mut r = sample();
        r.identities[0]
            .capabilities
            .extra
            .insert("custom_admin".into(), true);
        assert!(r.can("alice", "custom_admin"));
        assert!(!r.can("alice", "nonexistent"));
    }

    #[test]
    fn offboard_sets_status() {
        let mut r = sample();
        r.offboard("alice").unwrap();
        let alice = r.identities.iter().find(|i| i.id == "alice").unwrap();
        assert_eq!(alice.status, IdentityStatus::Offboarded);
    }

    #[test]
    fn offboard_unknown_id_errors() {
        let mut r = sample();
        let err = r.offboard("ghost").unwrap_err();
        assert!(matches!(err, IdentityError::Unresolved(_)));
    }
}
