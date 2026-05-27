//! # ft-pr
//!
//! `firetrail check pr` validation engine and the custom Git merge driver for
//! Firetrail record files.
//!
//! ## Surface
//!
//! - [`PrValidator`] / [`validate_pr`] — main validation entry point.
//! - [`PrValidatorOptions`] — knobs (strict, AC cap, draft expiry, secret patterns).
//! - [`PrReport`], [`PrFinding`], [`Severity`], [`RuleId`] — structured output.
//! - [`PrError`] — top-level error type for validation failures distinct from
//!   findings.
//! - [`ValidationCache`] / [`validate_pr_cached`] — content-hash caching for
//!   fast re-validation.
//! - [`merge`] — JSON merge driver for three-way merges of record files.
//!
//! ## Relevant ADRs
//!
//! - ADR-0009 — Memory-only PRs
//! - ADR-0010 — PR link enforcement
//! - ADR-0013 — Trust model
//! - ADR-0017 — Audit-chain integrity
//! - ADR-0003 — PR compaction history

#![allow(clippy::result_large_err)]

mod cache;
mod error;
mod options;
mod path;
mod report;
mod rules;
mod validator;

pub mod merge;

pub use cache::{ValidationCache, validate_pr_cached};
pub use error::PrError;
pub use options::PrValidatorOptions;
pub use report::{PrFinding, PrReport, PrSummary, RuleId, Severity};
pub use validator::{PrValidator, validate_pr};
