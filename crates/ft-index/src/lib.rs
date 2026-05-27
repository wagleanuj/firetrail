//! # ft-index
//!
//! SQLite-backed read index over the JSON-in-Git record store.
//!
//! `ft-index` is **derived data**: rebuildable from the source-of-truth JSON
//! files at any time. It exists because walking thousands of JSON files for
//! every list, ready-detection, or dependency-walk query is slow.
//!
//! ## M1 surface
//!
//! - [`Index`] — opens/creates `.firetrail/index.db`, applies migrations,
//!   serves the read queries.
//! - [`ListQuery`] / [`ReadyQuery`] / [`WalkDirection`] / [`OrderBy`] — query
//!   shapes.
//! - [`IndexedRecord`] / [`DepEdge`] — query results.
//! - [`RebuildReport`] / [`RefreshReport`] — write-path summaries.
//! - [`Storage`] / [`StorageError`] / [`StorageFilter`] — re-exported from the
//!   canonical `ft-storage` crate so downstream callers can keep importing
//!   them from `ft-index` while a single trait owns the contract.
//!
//! ## Relevant ADRs
//!
//! - ADR-0002 — JSON-in-Git, not Dolt (index is derived)
//! - ADR-0006 — Storage modes
//! - ADR-0007 — Local embeddings (same database holds vector tables in M3)
//! - ADR-0011 — Offline-first
//! - ADR-0015 — Hash-based IDs
//! - ADR-0017 — Audit-chain integrity

#![deny(missing_docs)]

mod error;
mod index;
mod schema;
mod types;

pub use error::IndexError;
pub use ft_storage::{Storage, StorageError, StorageFilter};
pub use index::Index;
pub use types::{
    DepEdge, IndexedRecord, ListQuery, OrderBy, ReadyQuery, RebuildReport, RefreshReport,
    WalkDirection,
};
