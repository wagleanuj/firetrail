//! # ft-trust
//!
//! Trust state machine, evidence tracking, and human review workflow for
//! Firetrail records. The full rule set is specified in ADR-0013; this crate
//! is the executable form of that ADR.
//!
//! ## Public surface
//!
//! - [`TrustTransition`] — the audit-trail unit; one per state move.
//! - [`TrustError`] — typed failure modes corresponding to ADR-0013 clauses.
//! - [`validate_transition`] — checks whether a requested move is legal given
//!   the current record, its author, and the reviewers that have already
//!   promoted it.
//! - [`apply_transition`] — mutates a [`MemoryBody`]'s `trust` field and
//!   performs redaction when transitioning to
//!   [`ft_core::TrustState::Redacted`]. Returns the (timestamp-fixed)
//!   transition for the caller to append to history.
//! - [`MemoryBody`] — mutable view over the memory-kind variants of
//!   [`ft_core::RecordBody`]; built via [`MemoryBody::from_record_body`].
//! - [`StalePolicy`], [`is_stale`] — per-kind age-based staleness rules.
//!
//! ## Identity / origin attribution
//!
//! The state machine *trusts* the [`ft_core::Origin`] flag set on each
//! [`TrustTransition`]. Distinguishing actual humans from bots is the job of
//! identity infrastructure (ADR-0008 / `ft-identity`). What this crate
//! enforces is: *if a transition is flagged as agent-originated, it cannot
//! promote to [`ft_core::TrustState::Verified`]*.
//!
//! ## State-hash chaining
//!
//! [`apply_transition`] does **not** touch the envelope's `state_hash` or
//! `prev_state_hash` — that's `ft-history`'s job. This crate only enforces
//! trust semantics on the body.
//!
//! ## Relevant ADRs
//!
//! - ADR-0008 — Identity registry
//! - ADR-0013 — Trust model
//! - ADR-0014 — Import quarantine
//! - ADR-0017 — Audit-chain integrity

pub mod body;
pub mod error;
pub mod policy;
pub mod state_machine;
pub mod transition;

pub use body::MemoryBody;
pub use error::TrustError;
pub use policy::{HIGH_STAKES_REVALIDATION_DAYS, StalePolicy, is_stale};
pub use state_machine::{
    VERIFIED_REVIEWER_COUNT, apply_transition, is_terminal, validate_transition,
};
pub use transition::TrustTransition;
