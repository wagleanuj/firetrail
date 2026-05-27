//! # ft-storage
//!
//! JSON-in-Git read/write storage. Supports both the embedded mode (records
//! co-located with the working repo) and the external mode (records stored
//! in a separate dedicated repository).
//!
//! ## Relevant ADRs
//!
//! - ADR-0002 — JSON-in-Git, not Dolt
//! - ADR-0006 — Storage modes (embedded vs external)
//! - ADR-0011 — Offline-first
