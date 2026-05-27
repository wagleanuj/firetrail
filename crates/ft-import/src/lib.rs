//! # ft-import
//!
//! Importers for external content (Markdown, Jira, Confluence) that produce
//! quarantined records pending human review (ADR-0014).
//!
//! ## Design choices
//!
//! - **Quarantine encoding**: a quarantined record carries the label
//!   `quarantine=true` (plus a sibling `import:source=<system>` label) rather
//!   than a dedicated envelope field. Adding an envelope field would touch the
//!   canonical JSON schema, the hash, every storage round-trip, and every
//!   downstream crate; a label is the smallest viable representation and
//!   serializes round-trip-cleanly through `ft-core`. See
//!   [`is_quarantined`] and [`QUARANTINE_LABEL_KEY`].
//! - **Markdown parsing**: a regex-free hand-rolled line scanner that detects
//!   `##` (and `#`) headers case-insensitively. The crate stays free of the
//!   `pulldown-cmark` dependency at M6; if a future importer needs full
//!   markdown semantics we can lift it in then.
//!
//! ## Relevant ADRs
//!
//! - ADR-0014 — Import quarantine
//! - ADR-0013 — Trust model
//! - ADR-0017 — Audit-chain integrity (promotion appends a history entry)

pub mod adapter;
pub mod convert;
pub mod error;
pub mod import;
pub mod parse;
pub mod promote;
pub mod quarantine;
pub mod source;

pub use adapter::{ImportAdapter, MockJiraAdapter, RawImport};
pub use convert::{
    BuilderOpts, parsed_adr_to_record, parsed_incident_to_record, parsed_runbook_to_record,
};
pub use error::ImportError;
pub use import::{ImportKind, ImportOptions, ImportReport, import_dir};
pub use parse::{
    ParsedAdr, ParsedIncident, ParsedRunbook, parse_adr_md, parse_incident_md, parse_runbook_md,
};
pub use promote::{PromotionCandidate, PromotionOpts, promote_record, promotion_candidates};
pub use quarantine::{
    IMPORT_SOURCE_LABEL_KEY, QUARANTINE_LABEL_KEY, QUARANTINE_LABEL_VALUE, is_quarantined,
};
pub use source::{ImportSource, SourceSystem};
