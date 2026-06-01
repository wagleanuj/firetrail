//! ft-ops — transport-agnostic firetrail command bodies.
//!
//! Forbidden inside ft-ops:
//! - `println!` / `eprintln!` / reading stdin
//! - `clap` or `axum` types
//! - `std::env::current_dir()` or other ambient context
//! - HTTP-specific or CLI-specific error types
//!
//! Every op is `fn op(ws: &Workspace, identity: &Identity, input: I, events: &EventBus)
//! -> Result<O, OpsError>`. Both ft-cli and ft-ui depend on this crate and adapt their
//! own transport (clap args / axum JSON) to call into it.
//!
//! See `docs/plans/2026-05-28-firetrail-gui-design.md` for the full design (Wave 0
//! delivers this scaffold; Waves 1–3 populate the domain modules by extracting
//! command bodies out of ft-cli).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod audit;
pub mod docs;
pub mod error;
pub mod events;
pub mod files;
pub mod identity;
pub mod identity_ops;
pub mod memory;
pub mod profile;
pub mod scope;
pub mod search;
pub mod synthetic_embed;
pub mod tickets;
pub mod trust;
pub mod workspace;

pub use error::OpsError;
pub use events::{EmittedEvent, Event, EventBus};
pub use identity::Identity;
pub use workspace::Workspace;
