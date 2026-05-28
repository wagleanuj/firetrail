//! Caller identity passed into every op.
//!
//! Wave 0 ships a minimal struct so the boundary types compile and ft-ui
//! can wire its session layer; later waves will extract the real identity
//! resolution out of `ft-identity` and replace this stub.

use serde::{Deserialize, Serialize};

/// Identity of the principal invoking an op.
///
/// In Wave 0 this is a thin record. Subsequent waves will replace it with
/// the resolved identity from `ft-identity` (key id, capabilities, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// Primary email used to attribute records.
    pub email: String,
    /// Display name.
    pub name: String,
}
