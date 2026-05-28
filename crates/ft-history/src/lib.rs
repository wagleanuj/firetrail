//! # ft-history
//!
//! PR-time history compaction and `prev_state_hash` chain management. Keeps
//! the per-record audit chain intact across squashes, rebases, and branch
//! salvage events.
//!
//! ## Surface
//!
//! - [`HistoryEntryKind`] — coarse classification of a history entry's
//!   semantics. Carried in [`HistoryDraft::kind`] and (after squash) collapsed
//!   into a single value per surviving entry.
//! - [`HistoryDraft`] — an in-flight history entry the caller hands to
//!   [`append_history`]. The crate fills in `state_hash` / `prev_state_hash`
//!   so the chain is always self-consistent.
//! - [`append_history`] — append a new entry, recompute the record's
//!   `state_hash`, and relink `prev_state_hash`.
//! - [`verify_chain`] — walk a record's `history[]` and check chain
//!   integrity, returning a precise [`VerifyError`] on the first break.
//! - [`CompactPolicy`] / [`compact_history`] / [`CompactReport`] — PR-time
//!   compaction (ADR-0003): squash consecutive same-author `Update` entries
//!   inside a window while preserving audit-critical kinds.
//!
//! ## Relevant ADRs
//!
//! - ADR-0003 — PR compaction history
//! - ADR-0017 — Audit-chain integrity
//! - ADR-0018 — Branch salvage

mod append;
mod compact;
mod error;
mod kind;
mod verify;

pub use append::{HistoryDraft, append_history};
pub use compact::{CompactPolicy, CompactReport, CompactedKind, compact_history, relink_chain};
pub use error::{HistoryError, VerifyError};
pub use kind::HistoryEntryKind;
pub use verify::verify_chain;
