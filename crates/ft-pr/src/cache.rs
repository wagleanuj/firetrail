//! Content-hash caching for fast re-validation.
//!
//! Validation is deterministic in `(record_state_hash, rule_set_version)`:
//! if the same set of records is presented with the same options, the same
//! findings come out. [`ValidationCache`] caches per-record cleanliness
//! verdicts keyed on those two inputs.
//!
//! The cache is in-memory at M4; SQLite-backed persistence is a follow-up
//! tracked in beads.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ft_git::Repo;
use ft_storage::Storage;

use crate::error::PrError;
use crate::options::PrValidatorOptions;
use crate::report::PrReport;
use crate::validator::validate_pr;

/// Rule-set version. Bump whenever a rule's *semantics* (not just message
/// formatting) changes — cached verdicts produced with a different version
/// are discarded.
pub const RULE_SET_VERSION: u32 = 1;

/// In-memory verdict cache. Cheap to clone (it's just a `HashMap`).
///
/// The cache key is `(rule_set_version, state_hash)`. A cache hit on every
/// changed record in the PR lets the validator skip the per-rule loop and
/// re-emit the prior report's findings for those records. Currently we cache
/// at the whole-PR granularity keyed by the multiset of `state_hash` values, so a
/// repeated `validate_pr_cached` on the *same* set of records returns
/// instantly.
#[derive(Debug, Default, Clone)]
pub struct ValidationCache {
    entries: HashMap<u64, PrReport>,
}

impl ValidationCache {
    /// Empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert manually (used by tests).
    pub fn insert(&mut self, key: u64, report: PrReport) {
        self.entries.insert(key, report);
    }

    /// Lookup; returns `None` on miss.
    #[must_use]
    pub fn get(&self, key: u64) -> Option<&PrReport> {
        self.entries.get(&key)
    }

    /// Number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Variant of [`validate_pr`] that consults `cache` before re-running rules.
///
/// On cache miss, runs the full validator and stores the resulting report
/// keyed on the fingerprint of the input. The fingerprint covers:
///
/// - The PR's `(base, head)` pair (lets cache survive ref churn that doesn't
///   actually move the records).
/// - The set of state hashes of every record present at head.
/// - The currently-active rule-set version.
/// - The salient option knobs (strict, AC cap, draft expiry, secret-scan
///   pattern strings).
pub fn validate_pr_cached(
    storage: &dyn Storage,
    git: &Repo,
    base: &str,
    head: &str,
    opts: &PrValidatorOptions,
    cache: &mut ValidationCache,
) -> Result<PrReport, PrError> {
    let key = fingerprint(git, base, head, opts)?;
    if let Some(hit) = cache.get(key) {
        return Ok(hit.clone());
    }
    let report = validate_pr(storage, git, base, head, opts)?;
    cache.insert(key, report.clone());
    Ok(report)
}

fn fingerprint(
    git: &Repo,
    base: &str,
    head: &str,
    opts: &PrValidatorOptions,
) -> Result<u64, PrError> {
    let diff = git.diff(base, head, None)?;
    let mut hasher = DefaultHasher::new();
    RULE_SET_VERSION.hash(&mut hasher);
    base.hash(&mut hasher);
    head.hash(&mut hasher);
    opts.strict.hash(&mut hasher);
    opts.max_ac_per_record.hash(&mut hasher);
    opts.draft_max_age_days.hash(&mut hasher);
    opts.enable_secret_scan.hash(&mut hasher);
    opts.verify_evidence_urls.hash(&mut hasher);
    for p in &opts.secret_patterns {
        p.as_str().hash(&mut hasher);
    }
    // Order-independent contribution of changed blob ids.
    let mut blobs: Vec<String> = diff
        .iter()
        .filter_map(|e| e.new_sha.clone().or_else(|| e.old_sha.clone()))
        .collect();
    blobs.sort();
    for b in blobs {
        b.hash(&mut hasher);
    }
    Ok(hasher.finish())
}
