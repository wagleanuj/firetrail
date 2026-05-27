//! Tunables for [`crate::validate_pr`].

use regex::Regex;

/// Knobs controlling how strict the PR validator is, plus the secret-scan
/// regex set.
///
/// The defaults reflect the ADR-driven baseline: AC cap of 10 (ADR-0013),
/// draft expiry at 14 days (ADR-0013), secret scanning enabled with a handful
/// of common API-key shapes, evidence URL fetches disabled at M4 (deferred to
/// follow-up work).
#[derive(Debug, Clone)]
pub struct PrValidatorOptions {
    /// Treat warnings as errors (causes [`crate::PrReport::is_clean`] to
    /// return `false` if any warnings are present).
    pub strict: bool,

    /// Maximum acceptance criteria per record before [`crate::RuleId::AcCapExceeded`]
    /// fires. ADR-0013 specifies a default of 10.
    pub max_ac_per_record: usize,

    /// Drafts older than this many days trigger
    /// [`crate::RuleId::DraftExpired`]. ADR-0013 specifies 14 days.
    pub draft_max_age_days: i64,

    /// Whether to run the secret scanner.
    pub enable_secret_scan: bool,

    /// Compiled regexes for the secret scanner. Replaces the default set if
    /// supplied; see [`default_secret_patterns`] for the baseline.
    pub secret_patterns: Vec<Regex>,

    /// Whether to fetch and validate evidence URLs. Disabled at M4 to avoid
    /// network dependence in CI; deferred to follow-up.
    pub verify_evidence_urls: bool,
}

impl Default for PrValidatorOptions {
    fn default() -> Self {
        Self {
            strict: false,
            max_ac_per_record: 10,
            draft_max_age_days: 14,
            enable_secret_scan: true,
            secret_patterns: default_secret_patterns(),
            verify_evidence_urls: false,
        }
    }
}

/// Default secret-scan regex set.
///
/// Patterns:
///
/// - AWS access key ids (`AKIA` + 16 base32 chars).
/// - GitHub personal access tokens (`ghp_` / `gho_` / `ghu_` / `ghs_` / `ghr_` + 36 base62 chars).
/// - Generic 32+ char hex strings inside surrounding quotes (matches many
///   embedded API key formats).
#[must_use]
pub fn default_secret_patterns() -> Vec<Regex> {
    let raw = [
        r"\bAKIA[0-9A-Z]{16}\b",
        r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
        r#"["'][0-9a-fA-F]{32,}["']"#,
    ];
    raw.iter().filter_map(|p| Regex::new(p).ok()).collect()
}
