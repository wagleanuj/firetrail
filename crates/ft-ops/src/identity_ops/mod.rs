//! Transport-agnostic identity-registry ops (Wave 3-A).
//!
//! Mirrors `ft_cli::commands::identity` but conforms to the ops boundary
//! contract: no `println!`, no clap, no stdin, no axum, no current-dir. Each
//! op takes `(&Workspace, &Identity, Input, &EventBus)` and returns
//! `Result<Output, OpsError>`.
//!
//! Inputs include an optional `request_id` that is threaded into every
//! emitted [`crate::Event`] so the GUI can coalesce optimistic updates with
//! the SSE echo.
//!
//! The directory is named `identity_ops` (not `identity`) so it doesn't
//! collide with [`crate::identity::Identity`] — the caller-identity type
//! threaded through every op.
//!
//! ft-cli's existing `identity` subcommands are NOT rewired here; that is
//! tracked under firetrail-xy6.

mod offboard;
mod register;
mod views;

pub use offboard::{IdentityOffboardOutput, OffboardInput, offboard};
pub use register::{
    CapabilitiesInput, CapabilitiesOutput, CapabilityOverrideInput, CapabilityRow,
    IdentityKindInput, IdentityRegisterOutput, RegisterInput, capabilities, register,
};
pub use views::{
    IdentityListOutput, IdentityShowOutput, IdentityStatusFilter, IdentityView, ListInput,
    ShowInput, list, show,
};
