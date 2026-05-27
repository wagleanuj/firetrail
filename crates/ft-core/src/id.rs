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

/// Failure modes for [`resolve_prefix`].
///
/// All variants are `Clone + Eq` so callers can assert on them in tests.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// The supplied prefix was empty or whitespace-only.
    #[error("empty record-id prefix")]
    Empty,

    /// A kind tag was present (e.g. `TASK-` or bare `TASK`) but no hex tail
    /// was supplied. Distinct from `Empty` so the CLI can surface a clearer
    /// user error than a generic not-found.
    #[error("hex prefix is required after kind tag `{0}`")]
    EmptyHexPrefix(String),

    /// No candidate matched the supplied prefix.
    #[error("no record matches prefix `{0}`")]
    Unknown(String),

    /// More than one candidate matched the supplied prefix.
    #[error(
        "prefix `{prefix}` is ambiguous; matches {n} records: {list}",
        n = matches.len(),
        list = matches.iter().map(RecordId::as_str).collect::<Vec<_>>().join(", "),
    )]
    Ambiguous {
        /// Original input (normalised lowercase) that produced the collision.
        prefix: String,
        /// All matching candidates, preserving input order.
        matches: Vec<RecordId>,
    },
}

/// Resolve a user-typed prefix against `candidates` into a single
/// [`RecordId`].
///
/// # Accepted forms
///
/// - **Full canonical id** (`KIND-<64 lowercase hex>`): if it parses via
///   [`RecordId::from_string`], membership in `candidates` is checked and the
///   id returned. Missing membership yields [`ResolveError::Unknown`].
/// - **Kind-prefixed prefix** (`KIND-<short hex>`, e.g. `TASK-abc`): the part
///   before `-` is matched case-insensitively against the uppercase kind tag
///   from [`RecordKind::prefix`]. The part after `-` must be a non-empty,
///   case-insensitive hex string that is matched against the tail of each
///   candidate of that kind.
/// - **Bare hex prefix** (e.g. `abc`): matched against the tail of every
///   candidate regardless of kind.
///
/// # Rules
///
/// - Empty / whitespace-only input → [`ResolveError::Empty`].
/// - Non-hex characters in the tail prefix → [`ResolveError::Unknown`].
/// - Unknown kind tag → [`ResolveError::Unknown`].
/// - Zero matches → [`ResolveError::Unknown`].
/// - One match → `Ok(id)`.
/// - Two-or-more matches → [`ResolveError::Ambiguous`].
///
/// # Errors
///
/// Returns [`ResolveError`] when the prefix cannot be uniquely resolved.
pub fn resolve_prefix(prefix: &str, candidates: &[RecordId]) -> Result<RecordId, ResolveError> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return Err(ResolveError::Empty);
    }

    // Fast path: full canonical id.
    if let Ok(full) = RecordId::from_string(trimmed.to_string()) {
        return candidates
            .iter()
            .find(|c| *c == &full)
            .cloned()
            .ok_or_else(|| ResolveError::Unknown(trimmed.to_string()));
    }

    // Parse `KIND-hex` vs bare hex.
    let (kind_filter, hex_prefix) = if let Some((kind_part, tail_part)) = trimmed.split_once('-') {
        let upper = kind_part.to_ascii_uppercase();
        let Some(kind) = RecordKind::from_prefix(&upper) else {
            return Err(ResolveError::Unknown(trimmed.to_string()));
        };
        // `KIND-` with empty/whitespace-only tail is a user error, not a
        // not-found.
        if tail_part.trim().is_empty() {
            return Err(ResolveError::EmptyHexPrefix(upper));
        }
        (Some(kind), tail_part)
    } else {
        // Bare input with no `-`. If it matches a known kind tag
        // (case-insensitive) AND is not itself a valid hex prefix, the
        // user meant `KIND-<hex>` but forgot the tail — same error as
        // `KIND-`. We only short-circuit on non-hex tokens so bare hex
        // inputs that happen to spell short kind tags (e.g. `DEC`,
        // `FED`) still match candidates the normal way.
        let upper = trimmed.to_ascii_uppercase();
        let is_hex = trimmed.bytes().all(|b| b.is_ascii_hexdigit());
        if !is_hex && RecordKind::from_prefix(&upper).is_some() {
            return Err(ResolveError::EmptyHexPrefix(upper));
        }
        (None, trimmed)
    };

    if hex_prefix.is_empty() {
        return Err(ResolveError::Unknown(trimmed.to_string()));
    }
    if !hex_prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ResolveError::Unknown(trimmed.to_string()));
    }
    let hex_lower = hex_prefix.to_ascii_lowercase();

    let matches: Vec<RecordId> = candidates
        .iter()
        .filter(|c| match kind_filter {
            Some(k) => c.kind() == k,
            None => true,
        })
        .filter(|c| {
            let tail = c.0.split_once('-').expect("RecordId invariant").1;
            tail.starts_with(&hex_lower)
        })
        .cloned()
        .collect();

    match matches.len() {
        0 => Err(ResolveError::Unknown(trimmed.to_string())),
        1 => Ok(matches.into_iter().next().expect("len==1")),
        _ => Err(ResolveError::Ambiguous {
            prefix: trimmed.to_ascii_lowercase(),
            matches,
        }),
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

    /// ADR-0015 acceptance: 16M+ unique IDs should not collide.
    /// Ignored by default; run via `cargo test -p ft-core -- --ignored mint_is_unique_at_scale`
    /// in nightly CI.
    #[test]
    #[ignore = "nightly stress test; ~16M mints, runs minutes"]
    fn mint_is_unique_at_scale() {
        let alice = alice();
        let n: usize = 16 * 1024 * 1024;
        let mut set = std::collections::HashSet::with_capacity(n);
        for _ in 0..n {
            let id = RecordId::mint(RecordKind::Task, &alice);
            assert!(set.insert(id), "duplicate id minted within {n} samples");
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

    fn task(hex_tail: &str) -> RecordId {
        RecordId::from_string(format!("TASK-{hex_tail}")).unwrap()
    }

    fn epic(hex_tail: &str) -> RecordId {
        RecordId::from_string(format!("EPIC-{hex_tail}")).unwrap()
    }

    #[test]
    fn resolve_prefix_full_id_resolves() {
        let id = task(&"a".repeat(64));
        let other = epic(&"b".repeat(64));
        let candidates = vec![id.clone(), other];
        let got = resolve_prefix(id.as_str(), &candidates).unwrap();
        assert_eq!(got, id);
    }

    #[test]
    fn resolve_prefix_full_id_not_in_candidates_is_unknown() {
        let id = task(&"a".repeat(64));
        let candidates: Vec<RecordId> = vec![];
        assert_eq!(
            resolve_prefix(id.as_str(), &candidates),
            Err(ResolveError::Unknown(id.as_str().to_string()))
        );
    }

    #[test]
    fn resolve_prefix_bare_hex_unique_match() {
        let a = task(&format!("abc123{}", "0".repeat(58)));
        let b = epic(&format!("ffeedd{}", "0".repeat(58)));
        let candidates = vec![a.clone(), b];
        let got = resolve_prefix("abc12", &candidates).unwrap();
        assert_eq!(got, a);
    }

    #[test]
    fn resolve_prefix_kind_filter_excludes_other_kinds() {
        let t = task(&format!("abc123{}", "0".repeat(58)));
        let e = epic(&format!("abc123{}", "1".repeat(58)));
        let candidates = vec![t.clone(), e];
        let got = resolve_prefix("TASK-abc", &candidates).unwrap();
        assert_eq!(got, t);
    }

    #[test]
    fn resolve_prefix_kind_filter_is_case_insensitive() {
        let t = task(&format!("abc123{}", "0".repeat(58)));
        let candidates = vec![t.clone()];
        let got = resolve_prefix("task-abc", &candidates).unwrap();
        assert_eq!(got, t);
    }

    #[test]
    fn resolve_prefix_ambiguous_lists_matches() {
        let a = task(&format!("abc123{}", "0".repeat(58)));
        let b = task(&format!("abc123{}", "1".repeat(58)));
        let candidates = vec![a.clone(), b.clone()];
        let err = resolve_prefix("abc123", &candidates).unwrap_err();
        match err {
            ResolveError::Ambiguous { prefix, matches } => {
                assert_eq!(prefix, "abc123");
                assert_eq!(matches.len(), 2);
                let msg = format!(
                    "{}",
                    ResolveError::Ambiguous {
                        prefix: prefix.clone(),
                        matches: matches.clone(),
                    }
                );
                assert!(msg.contains(a.as_str()));
                assert!(msg.contains(b.as_str()));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_prefix_unknown_when_no_match() {
        let candidates = vec![task(&"a".repeat(64))];
        assert_eq!(
            resolve_prefix("deadbe", &candidates),
            Err(ResolveError::Unknown("deadbe".to_string()))
        );
    }

    #[test]
    fn resolve_prefix_empty_input() {
        let candidates: Vec<RecordId> = vec![];
        assert_eq!(resolve_prefix("", &candidates), Err(ResolveError::Empty));
        assert_eq!(
            resolve_prefix("   \t", &candidates),
            Err(ResolveError::Empty)
        );
    }

    #[test]
    fn resolve_prefix_case_insensitive_hex() {
        let t = task(&format!("abcdef{}", "0".repeat(58)));
        let candidates = vec![t.clone()];
        let got = resolve_prefix("ABCDEF", &candidates).unwrap();
        assert_eq!(got, t);
        let got2 = resolve_prefix("TASK-ABCDEF", &candidates).unwrap();
        assert_eq!(got2, t);
    }

    #[test]
    fn resolve_prefix_rejects_non_hex_tail() {
        let t = task(&"a".repeat(64));
        let candidates = vec![t];
        assert!(matches!(
            resolve_prefix("xyz", &candidates),
            Err(ResolveError::Unknown(_))
        ));
        assert!(matches!(
            resolve_prefix("TASK-xyz", &candidates),
            Err(ResolveError::Unknown(_))
        ));
    }

    #[test]
    fn resolve_prefix_kind_only_with_no_hex_is_user_error() {
        // Regression for firetrail-58h: `TASK-` and `TASK` are kind-only
        // inputs and must surface as a distinct error variant, not
        // Unknown/NotFound.
        let t = task(&"a".repeat(64));
        let candidates = vec![t];
        assert_eq!(
            resolve_prefix("TASK-", &candidates),
            Err(ResolveError::EmptyHexPrefix("TASK".to_string()))
        );
        assert_eq!(
            resolve_prefix("TASK- ", &candidates),
            Err(ResolveError::EmptyHexPrefix("TASK".to_string()))
        );
        assert_eq!(
            resolve_prefix("task-", &candidates),
            Err(ResolveError::EmptyHexPrefix("TASK".to_string()))
        );
        assert_eq!(
            resolve_prefix("TASK", &candidates),
            Err(ResolveError::EmptyHexPrefix("TASK".to_string()))
        );
        assert_eq!(
            resolve_prefix("task", &candidates),
            Err(ResolveError::EmptyHexPrefix("TASK".to_string()))
        );
    }

    #[test]
    fn resolve_prefix_unknown_kind_tag() {
        let t = task(&"a".repeat(64));
        let candidates = vec![t];
        assert!(matches!(
            resolve_prefix("BOGUS-abc", &candidates),
            Err(ResolveError::Unknown(_))
        ));
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
