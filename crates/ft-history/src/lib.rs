//! # ft-history
//!
//! PR-time history compaction and `prev_state_hash` chain management. Keeps
//! the per-record audit chain intact across squashes, rebases, and branch
//! salvage events.
//!
//! ## Relevant ADRs
//!
//! - ADR-0003 — PR compaction history
//! - ADR-0017 — Audit-chain integrity
//! - ADR-0018 — Branch salvage
