//! Record kind and record identifier types.
//!
//! Per ADR-0015, a record ID is the prefix-tagged full SHA-256 of a content
//! derivation material, displayed via adaptive prefix length but stored and
//! transmitted as the full string.

use std::collections::HashMap;

use rand::RngCore;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::CoreError;
use crate::identity::Identity;

/// Every record type Firetrail supports.
///
/// M1 implements all listed variants; M2 enables `RecordBuilder` for the
/// memory kinds (`Incident`, `Finding`, `Runbook`, `Decision`, `Gotcha`,
/// `Memory`). All variants round-trip through serde at M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// Long-lived effort grouping tasks.
    Epic,
    /// A unit of planned work.
    Task,
    /// A child of a task.
    Subtask,
    /// A defect record.
    Bug,
    /// Incident report (memory kind, writable from M2).
    Incident,
    /// Investigative finding (memory kind, writable from M2).
    Finding,
    /// Operational runbook (memory kind, writable from M2).
    Runbook,
    /// Architectural / design decision (memory kind, writable from M2).
    Decision,
    /// Recurring footgun (memory kind, writable from M2).
    Gotcha,
    /// Generic memory note (memory kind, writable from M2).
    Memory,
}

impl RecordKind {
    /// Uppercase prefix used in the display form of a record ID.
    #[must_use]
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Task => "TASK",
            Self::Epic => "EPIC",
            Self::Bug => "BUG",
            Self::Subtask => "SUB",
            Self::Incident => "INC",
            Self::Finding => "FIND",
            Self::Runbook => "RUN",
            Self::Decision => "DEC",
            Self::Gotcha => "GOTCHA",
            Self::Memory => "MEM",
        }
    }

    /// Parse a record-id prefix back into its kind.
    fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "TASK" => Some(Self::Task),
            "EPIC" => Some(Self::Epic),
            "BUG" => Some(Self::Bug),
            "SUB" => Some(Self::Subtask),
            "INC" => Some(Self::Incident),
            "FIND" => Some(Self::Finding),
            "RUN" => Some(Self::Runbook),
            "DEC" => Some(Self::Decision),
            "GOTCHA" => Some(Self::Gotcha),
            "MEM" => Some(Self::Memory),
            _ => None,
        }
    }
}

/// A canonical record identifier (ADR-0015).
///
/// Stored as `<KIND>-<64 lowercase hex chars>`. The hex portion is the full
/// SHA-256 of the record-creation material; the kind prefix is uppercase in
/// display contexts (the on-disk path uses the lowercase form, applied by
/// `ft-storage`).
///
/// # Examples
///
/// ```
/// use ft_core::{Identity, RecordId, RecordKind};
///
/// let alice = Identity::new("alice@example.com").unwrap();
/// let id = RecordId::mint(RecordKind::Task, &alice);
/// assert!(id.as_str().starts_with("TASK-"));
/// assert_eq!(id.short(6).len(), 11); // "TASK-" + 6 hex chars
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct RecordId(String);

/// Minimum display prefix length (ADR-0015).
pub const MIN_DISPLAY_PREFIX: usize = 6;

/// Length in hex characters of the SHA-256 hash portion.
pub const HASH_HEX_LEN: usize = 64;

impl RecordId {
    /// Mint a new `RecordId` from creation context per ADR-0015.
    ///
    /// Derivation material: `nonce | identity | kind | timestamp_ms`, SHA-256,
    /// lowercase hex. The kind prefix is prepended in uppercase form.
    pub fn mint(kind: RecordKind, identity: &Identity) -> Self {
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);
        let nonce_hex = hex::encode(nonce);
        let kind_json = serde_json::to_string(&kind).expect("RecordKind always serializes");
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let material = format!(
            "{nonce_hex}|{identity}|{kind_json}|{timestamp}",
            identity = identity.as_str()
        );
        let digest = Sha256::digest(material.as_bytes());
        let hex = hex::encode(digest);
        Self(format!("{}-{}", kind.prefix(), hex))
    }

    /// Construct from an existing string. Validates structure only; does not
    /// re-derive.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidId`] if the prefix is unknown, the
    /// separator is missing, or the hex tail is not 64 lowercase hex chars.
    pub fn from_string(s: impl Into<String>) -> Result<Self, CoreError> {
        let s = s.into();
        let Some((prefix, tail)) = s.split_once('-') else {
            return Err(CoreError::InvalidId(format!(
                "missing `-` separator in `{s}`"
            )));
        };
        if RecordKind::from_prefix(prefix).is_none() {
            return Err(CoreError::InvalidId(format!(
                "unknown record kind prefix `{prefix}`"
            )));
        }
        if tail.len() != HASH_HEX_LEN
            || !tail.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(CoreError::InvalidId(format!(
                "expected {HASH_HEX_LEN} lowercase hex chars after prefix in `{s}`"
            )));
        }
        Ok(Self(s))
    }

    /// Borrow the full canonical string form.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the `RecordKind` parsed from this id's prefix.
    ///
    /// Always succeeds for ids constructed via [`Self::mint`] or
    /// [`Self::from_string`].
    #[must_use]
    pub fn kind(&self) -> RecordKind {
        let (prefix, _) = self
            .0
            .split_once('-')
            .expect("RecordId invariant: contains `-`");
        RecordKind::from_prefix(prefix).expect("RecordId invariant: known prefix")
    }

    /// Display form truncated to `len` hex characters after the prefix.
    ///
    /// Returns the entire prefix (`KIND-`) plus the first `len` hex chars
    /// from the hash tail. `len` is clamped to the available hex length.
    #[must_use]
    pub fn short(&self, len: usize) -> &str {
        let (prefix, tail) = self
            .0
            .split_once('-')
            .expect("RecordId invariant: contains `-`");
        let take = len.min(tail.len());
        // SAFETY of indexing: tail is ASCII hex.
        let end = prefix.len() + 1 + take;
        &self.0[..end]
    }

    /// Return the shortest display form of this id that is unambiguous within
    /// the supplied candidate set, clamped to at least
    /// [`MIN_DISPLAY_PREFIX`] hex characters (ADR-0015).
    ///
    /// `candidates` is the set of all currently-known record ids in the
    /// current view. The result is `<KIND>-<n hex chars>` where `n` is the
    /// minimum length such that no other candidate of the same kind shares
    /// the same prefix.
    #[must_use]
    pub fn unambiguous_display(&self, candidates: &[Self]) -> String {
        let self_kind = self.kind();
        let self_tail = self.0.split_once('-').expect("invariant").1;

        let peers: Vec<&str> = candidates
            .iter()
            .filter(|c| c.0 != self.0 && c.kind() == self_kind)
            .map(|c| c.0.split_once('-').expect("invariant").1)
            .collect();

        let mut n = MIN_DISPLAY_PREFIX;
        while n < self_tail.len() {
            let prefix = &self_tail[..n];
            if peers.iter().all(|p| !p.starts_with(prefix)) {
                break;
            }
            n += 1;
        }
        format!("{}-{}", self_kind.prefix(), &self_tail[..n])
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Build an in-memory prefix table from a candidate set.
///
/// Useful for batch list-rendering: compute once, look up many.
#[must_use]
pub fn build_display_table(candidates: &[RecordId]) -> HashMap<RecordId, String> {
    candidates
        .iter()
        .map(|id| (id.clone(), id.unambiguous_display(candidates)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice() -> Identity {
        Identity::new("alice@example.com").unwrap()
    }

    #[test]
    fn mint_produces_well_formed_id() {
        let id = RecordId::mint(RecordKind::Task, &alice());
        assert!(id.as_str().starts_with("TASK-"));
        let tail = &id.as_str()[5..];
        assert_eq!(tail.len(), HASH_HEX_LEN);
        assert!(
            tail.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn mint_is_unique_across_many_calls() {
        let alice = alice();
        let mut set = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let id = RecordId::mint(RecordKind::Task, &alice);
            assert!(set.insert(id), "duplicate id minted");
        }
    }

    #[test]
    fn from_string_roundtrips() {
        let id = RecordId::mint(RecordKind::Bug, &alice());
        let again = RecordId::from_string(id.as_str().to_string()).unwrap();
        assert_eq!(id, again);
    }

    #[test]
    fn from_string_rejects_bad_inputs() {
        assert!(RecordId::from_string("not-an-id").is_err());
        assert!(RecordId::from_string("TASK-zz").is_err());
        assert!(RecordId::from_string("TASK").is_err());
        assert!(RecordId::from_string(format!("XYZ-{}", "a".repeat(64))).is_err());
        // uppercase hex tail rejected
        assert!(RecordId::from_string(format!("TASK-{}", "A".repeat(64))).is_err());
    }

    #[test]
    fn short_returns_prefix_plus_n_hex() {
        let id = RecordId::mint(RecordKind::Task, &alice());
        let s = id.short(6);
        assert_eq!(s.len(), "TASK-".len() + 6);
        assert!(s.starts_with("TASK-"));
    }

    #[test]
    fn kind_extracted_from_id() {
        let id = RecordId::mint(RecordKind::Epic, &alice());
        assert_eq!(id.kind(), RecordKind::Epic);
    }

    #[test]
    fn unambiguous_display_grows_prefix_until_unique() {
        // Hand-construct three TASK ids sharing a 6-hex prefix.
        let id_a = RecordId::from_string(format!("TASK-{}{}", "abcdef", "0".repeat(58))).unwrap();
        let id_b = RecordId::from_string(format!("TASK-{}{}", "abcdef", "1".repeat(58))).unwrap();
        let id_c = RecordId::from_string(format!("TASK-{}{}", "999999", "0".repeat(58))).unwrap();
        let candidates = vec![id_a.clone(), id_b.clone(), id_c.clone()];

        // id_c is unique at 6 chars.
        assert_eq!(id_c.unambiguous_display(&candidates), "TASK-999999");

        // id_a vs id_b need 7 chars to disambiguate.
        let disp_a = id_a.unambiguous_display(&candidates);
        let disp_b = id_b.unambiguous_display(&candidates);
        assert_ne!(disp_a, disp_b);
        assert!(disp_a.len() > "TASK-abcdef".len());
        assert!(disp_b.len() > "TASK-abcdef".len());
    }

    #[test]
    fn display_table_matches_per_id_call() {
        let ids: Vec<RecordId> = (0..5)
            .map(|_| RecordId::mint(RecordKind::Task, &alice()))
            .collect();
        let table = build_display_table(&ids);
        for id in &ids {
            assert_eq!(table[id], id.unambiguous_display(&ids));
        }
    }
}
