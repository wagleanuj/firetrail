//! Capability matrix and identity-kind defaults (ADR-0008, ADR-0013).
//!
//! Every [`crate::registry::RegisteredIdentity`] has an [`IdentityKind`] which
//! determines its default [`CapabilityMatrix`]. The registry file may then
//! override individual capabilities per identity.
//!
//! The capability surface here is the minimum required to support the
//! policies that already exist in ADRs:
//!
//! - `can_promote_verified` — promote a record's trust to Verified (ADR-0013).
//!   Humans default true, bots and CI default false.
//! - `can_close_high_risk` — close a record marked high-risk.
//! - `can_force_push` — admin-only override; defaults to false for every kind.
//! - `can_redact` — perform a redaction of historical content.
//! - `extra` — open-ended escape hatch for custom team policies.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Kind of an identity. Drives the default capability matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IdentityKind {
    /// Real person.
    #[default]
    Human,
    /// Service account / bot.
    Bot,
    /// Continuous integration runner.
    Ci,
}

/// Lifecycle status of an identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IdentityStatus {
    /// Active member; capabilities apply.
    #[default]
    Active,
    /// Departed; cannot hold live claims. Sweep job releases their claims.
    Offboarded,
}

/// The full effective capability matrix for an identity.
///
/// Built by composing kind defaults (via [`CapabilityMatrix::defaults_for_kind`])
/// with per-identity overrides from the registry file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CapabilityMatrix {
    /// Promote a record's trust state to Verified (ADR-0013).
    pub can_promote_verified: bool,
    /// Close a record flagged high-risk.
    pub can_close_high_risk: bool,
    /// Force-push to a protected branch. Admin only.
    pub can_force_push: bool,
    /// Redact historical record content (PII / leaked secrets).
    pub can_redact: bool,
    /// Custom named capabilities. Reserved for team policies the standard
    /// matrix does not anticipate.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, bool>,
}

impl CapabilityMatrix {
    /// Kind-default matrix.
    ///
    /// - [`IdentityKind::Human`] — every well-known capability is true except
    ///   `can_force_push` (admin only).
    /// - [`IdentityKind::Bot`] — `can_promote_verified` and `can_force_push`
    ///   and `can_redact` are false; `can_close_high_risk` is true so bots
    ///   can resolve incidents they own.
    /// - [`IdentityKind::Ci`] — same as bot.
    #[must_use]
    pub fn defaults_for_kind(kind: IdentityKind) -> Self {
        match kind {
            IdentityKind::Human => Self {
                can_promote_verified: true,
                can_close_high_risk: true,
                can_force_push: false,
                can_redact: true,
                extra: HashMap::new(),
            },
            IdentityKind::Bot | IdentityKind::Ci => Self {
                can_promote_verified: false,
                can_close_high_risk: true,
                can_force_push: false,
                can_redact: false,
                extra: HashMap::new(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_defaults() {
        let c = CapabilityMatrix::defaults_for_kind(IdentityKind::Human);
        assert!(c.can_promote_verified);
        assert!(c.can_close_high_risk);
        assert!(!c.can_force_push);
        assert!(c.can_redact);
    }

    #[test]
    fn bot_defaults() {
        let c = CapabilityMatrix::defaults_for_kind(IdentityKind::Bot);
        assert!(!c.can_promote_verified);
        assert!(c.can_close_high_risk);
        assert!(!c.can_force_push);
        assert!(!c.can_redact);
    }

    #[test]
    fn ci_defaults_match_bot() {
        let bot = CapabilityMatrix::defaults_for_kind(IdentityKind::Bot);
        let ci = CapabilityMatrix::defaults_for_kind(IdentityKind::Ci);
        assert_eq!(bot, ci);
    }
}
