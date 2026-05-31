//! # ft-core
//!
//! Core record types, canonical JSON serialization, JSON Schema validation,
//! and the hash-chain primitives that underpin every Firetrail record.
//!
//! `ft-core` is the foundation of Wave 1. Every other crate depends on the
//! types defined here. It has no I/O — only types, pure functions, and
//! validation. Persistence, indexing, history compaction, identity
//! resolution, and trust enforcement live in sibling crates.
//!
//! ## Surface
//!
//! - [`Record`] — envelope + body, the canonical Firetrail record
//! - [`RecordKind`], [`RecordId`], [`Identity`] — the identifying types
//! - [`builder::RecordBuilder`] — validating constructor
//! - [`hash::state_hash`], [`hash::canonical_json`] — deterministic hashing
//! - [`schema::record_schema`], [`schema::validate_record_json`] — schema gate
//!
//! ## Relevant ADRs
//!
//! - ADR-0002 — JSON-in-Git as the canonical store
//! - ADR-0003 — PR-time history compaction
//! - ADR-0004 — Multi-scope records
//! - ADR-0008 — Identity registry
//! - ADR-0013 — Trust model and `origin` field
//! - ADR-0015 — Hash-based record IDs
//! - ADR-0017 — Audit-chain integrity (`state_hash` / `prev_state_hash`)

pub mod acceptance;
pub mod builder;
pub mod enums;
pub mod error;
pub mod hash;
pub mod id;
pub mod identity;
pub mod label;
pub mod record;
pub mod relation;
pub mod schema;

pub use acceptance::{AcceptanceCriterion, Claim, Evidence};
pub use builder::RecordBuilder;
pub use enums::{
    AcStatus, DecisionStatus, EvidenceKind, Origin, Priority, RelationKind, RiskClass, Severity,
    Status, TrustState,
};
pub use error::CoreError;
pub use hash::{canonical_json, state_hash};
pub use id::{
    HASH_HEX_LEN, MIN_DISPLAY_PREFIX, RecordId, RecordKind, ResolveError, build_display_table,
    resolve_prefix,
};
pub use identity::Identity;
pub use label::{HistoryEntry, Label, Transition};
pub use record::{
    Bug, ComponentRef, Decision, Doc, Epic, Finding, Gotcha, Incident, Memory, Record, RecordBody,
    RecordEnvelope, RepoProfileBody, Runbook, RunbookStep, Subtask, Task,
};
pub use relation::Relation;
pub use schema::{record_schema, record_schema_json, validate_record_json};
