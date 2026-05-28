//! Transport-agnostic ticket lifecycle ops.
//!
//! Every op in this module conforms to the boundary contract spelled out in
//! `crate`-level docs: no `println!`, no clap, no stdin, no axum. Each op takes
//! `(&Workspace, &Identity, Input, &EventBus)` and returns `Result<Output,
//! OpsError>`. Writes always:
//!
//! - lift through a lockfile under `.firetrail/locks/<id>.<op>` for atomicity,
//! - append a per-record history entry via `ft-history` (`state_hash` chain),
//! - upsert the search FTS index (`ft-search`),
//! - best-effort dispatch an embed-on-write request to the daemon socket,
//! - publish an `Event` on the [`crate::EventBus`] (see [`crate::events::Event`]).
//!
//! Identity strict-mode (see `ft-identity::DefaultResolver`) is enforced on
//! every write op and rejected with [`crate::OpsError::PermissionDenied`].
//!
//! The 8 ticket ops exposed here are the input set for Wave 1-B of the
//! firetrail GUI design (see `docs/plans/2026-05-28-firetrail-gui-design.md`).
//! ft-cli's existing ticket commands are NOT rewired in this commit; that is
//! tracked as a separate beads follow-up.

mod board;
mod claim;
mod close;
mod create;
mod ctx;
mod link;
mod list;
mod show;
mod update;

pub use board::{BoardCard, BoardInput, BoardOutput, board};
pub use claim::{
    ClaimInput, ClaimOutput, UnclaimInput, UnclaimOutput, claim, unclaim,
};
pub use close::{CloseInput, CloseOutput, close};
pub use create::{
    CreateBugInput, CreateEpicInput, CreateSubtaskInput, CreateTaskInput, CreatedTicket,
    TicketPriority, create_bug, create_epic, create_subtask, create_task,
};
pub use link::{LinkInput, LinkOutput, TicketRelationKind, link};
pub use list::{ListInput, ListOutput, ListedTicket, TicketKindFilter, TicketStatusFilter, list};
pub use show::{ShowInput, ShowOutput, show};
pub use update::{UpdateInput, UpdateOutput, update};
