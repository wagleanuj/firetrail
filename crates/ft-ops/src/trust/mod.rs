//! Transport-agnostic trust-state ops over memory records (Wave 3-A).
//!
//! Mirrors `ft_cli::commands::trust`. Each op resolves the actor, reads the
//! target memory record, builds a `ft_trust::TrustTransition`, validates it
//! (ADR-0013 enforcement lives in `ft-trust`), and persists with a
//! `HistoryEntryKind::TrustTransition` entry. Validation failure leaves the
//! on-disk record untouched.
//!
//! Every op emits [`crate::Event::TrustTransitioned`] (and [`crate::Event::MemoryWritten`]
//! since the record was mutated). The interactive CLI surface (no prompts here
//! by contract) maps directly onto the typed inputs below.
//!
//! ft-cli's existing `memory review|promote|deprecate|archive|supersede|merge|redact`
//! commands are NOT rewired here; that is tracked under firetrail-xy6.

mod ops;

pub use ops::{
    EvidenceKindInput, MergeInput, MergeOutput, PromoteInput, ReasonInput, ReviewInput,
    SupersedeInput, TrustOutput, archive, deprecate, merge, promote, redact, review, supersede,
};
