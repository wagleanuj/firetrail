//! Identity newtype.
//!
//! M1 form per ADR-0008: a single canonical string carrying the resolved email
//! (or `external:<email>` for non-registered identities). M5 extends this with
//! `kind`, `status`, and capabilities.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Canonical identity reference (ADR-0008, M1 form).
///
/// The wire format is a non-empty UTF-8 string. Construction validates that
/// the value is non-empty and contains no whitespace.
///
/// # Examples
///
/// ```
/// use ft_core::Identity;
///
/// let alice = Identity::new("alice@example.com").unwrap();
/// assert_eq!(alice.as_str(), "alice@example.com");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Identity(String);

impl Identity {
    /// Construct an `Identity` from a string, validating shape.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidIdentity`] if the value is empty, contains
    /// only whitespace, or contains internal whitespace characters.
    pub fn new(s: impl Into<String>) -> Result<Self, CoreError> {
        let s = s.into();
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(CoreError::InvalidIdentity("identity is empty".into()));
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(CoreError::InvalidIdentity(format!(
                "identity `{s}` contains internal whitespace"
            )));
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Borrow the underlying canonical string.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty() {
        assert!(Identity::new("").is_err());
        assert!(Identity::new("   ").is_err());
    }

    #[test]
    fn rejects_internal_whitespace() {
        assert!(Identity::new("alice smith").is_err());
        assert!(Identity::new("alice\tsmith").is_err());
    }

    #[test]
    fn accepts_email() {
        let id = Identity::new("alice@example.com").unwrap();
        assert_eq!(id.as_str(), "alice@example.com");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let id = Identity::new("  alice@example.com  ").unwrap();
        assert_eq!(id.as_str(), "alice@example.com");
    }
}
