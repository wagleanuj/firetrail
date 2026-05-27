//! # ft-trust
//!
//! Trust state machine, evidence tracking, and human review workflow for
//! Firetrail records. Encodes valid trust-state transitions as types where
//! possible so the compiler catches invalid moves.
//!
//! ## Relevant ADRs
//!
//! - ADR-0013 — Trust model
//! - ADR-0014 — Import quarantine
