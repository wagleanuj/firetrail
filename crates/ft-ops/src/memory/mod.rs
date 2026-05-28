//! Transport-agnostic memory ops (Wave 2-A).
//!
//! Every op in this module conforms to the same boundary contract as
//! [`crate::tickets`]: no `println!`, no clap, no stdin reads, no axum. Each
//! op takes `(&Workspace, &Identity, Input, &EventBus)` and returns
//! `Result<Output, OpsError>`.
//!
//! Modules:
//!
//! - [`mod@views`] — read-only list / stale / show.
//! - [`mod@create`] — `incident|finding|runbook|decision|gotcha|memory
//!   create` plus the polymorphic `capture` op.
//! - [`mod@salvage`] — ADR-0018 branch-diff salvage. The CLI's interactive
//!   per-record `accept/reject/quit` prompt is replaced by an explicit
//!   `selected` accept-list on the input (callers pre-compute the
//!   candidate set with `dry_run = true`, then submit the selection).
//! - [`mod@search`] — `search` (lexical / hybrid / vector) + `similar`.
//!   When a vector-flavoured mode is requested the daemon is auto-spawned
//!   via a private helper; if it fails to come up, the op degrades to
//!   lexical and reports the degradation in `warnings`.
//!
//! Inputs include an optional `request_id` that is threaded into every
//! emitted [`crate::Event`] so the GUI can coalesce optimistic updates with
//! the SSE echo (Wave 1-B convention).
//!
//! ft-cli's existing `memory_*` / `search` / `salvage` commands are NOT
//! rewired in this commit; that is tracked under firetrail-xy6.

mod ctx;
mod daemon;

pub mod create;
pub mod salvage;
pub mod search;
pub mod views;

pub use create::{
    CaptureInput, CreateDecisionInput, CreateFindingInput, CreateGotchaInput,
    CreateIncidentInput, CreateMemoryInput, CreateRunbookInput, CreatedMemory, MemoryKind,
    RiskClassInput, SeverityInput, capture, create_decision, create_finding, create_gotcha,
    create_incident, create_memory, create_runbook,
};
pub use salvage::{
    SalvageEntry, SalvageEntryAction, SalvageInput, SalvageOutput, salvage,
};
pub use search::{
    SearchHitOut, SearchInput, SearchMode, SearchOutput, SimilarInput, search, similar,
};
pub use views::{
    ListInput, ListOutput, MemoryRowOut, ShowInput, ShowOutput, StaleInput, TrustStateInput,
    list, show, stale,
};
