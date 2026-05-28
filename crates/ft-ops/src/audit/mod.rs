//! Transport-agnostic audit ops (Wave 3-A).
//!
//! Surface:
//!
//! - [`lint()`] — static lint over the current workspace state.
//! - [`verify()`] — walk per-record history chains and report tampering.
//! - [`review()`] — read-only review summary for a single record.
//! - `criteria_*` — acceptance-criteria CRUD against ticket records.
//! - [`diff()`] — record-aware diff between two git refs.
//! - [`graph()`] — dependency-graph traversal returning nodes + edges.
//!
//! Every op conforms to the ops boundary contract: no `println!`, no clap,
//! no stdin, no axum. Inputs include an optional `request_id` propagated
//! into emitted events.
//!
//! ft-cli's corresponding command bodies are NOT rewired here; that is
//! tracked under firetrail-xy6.

pub mod criteria;
pub mod diff;
pub mod graph;
pub mod lint;
pub mod review;
pub mod verify;

pub use criteria::{
    CriteriaAddInput, CriteriaEvidenceInput, CriteriaListInput, CriteriaListOutput,
    CriteriaListRow, CriteriaToggleInput, CriteriaWriteOutput, criteria_add, criteria_check,
    criteria_evidence, criteria_list, criteria_uncheck,
};
pub use diff::{DiffChange, DiffInput, DiffOutput, DiffRow, diff};
pub use graph::{GraphDirectionInput, GraphEdge, GraphInput, GraphNode, GraphOutput, graph};
pub use lint::{LintFinding, LintInput, LintOutput, LintSeverity, lint};
pub use review::{
    ReviewAcRow, ReviewEvidenceRow, ReviewHistoryRow, ReviewInput, ReviewOutput, review,
};
pub use verify::{VerifyInput, VerifyOutput, VerifyResult, verify};
