//! Caller identity passed into every op.
//!
//! `ft-ops::Identity` is the transport-agnostic shape passed in by adapters
//! (ft-cli resolves it via `ft_identity::DefaultResolver`; ft-ui pulls it
//! off the authenticated session). Conversion into the validating
//! [`ft_core::Identity`] happens inside ops via [`Identity::to_core`].

use serde::{Deserialize, Serialize};

use crate::error::OpsError;

/// Identity of the principal invoking an op.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// Canonical identifier (typically an email; may be a short id from the
    /// identity registry).
    pub email: String,
    /// Display name (informational; not used for permission checks).
    pub name: String,
}

impl Identity {
    /// Build a new transport identity. Trims whitespace but does not validate
    /// shape — call [`Self::to_core`] when validation is needed.
    pub fn new(email: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            name: name.into(),
        }
    }

    /// Convert into the validating [`ft_core::Identity`].
    ///
    /// Returns [`OpsError::Validation`] when the email is empty or contains
    /// internal whitespace (mirrors `ft_core::Identity::new`).
    pub fn to_core(&self) -> Result<ft_core::Identity, OpsError> {
        ft_core::Identity::new(self.email.clone())
            .map_err(|e| OpsError::validation("identity.email", e.to_string()))
    }
}
