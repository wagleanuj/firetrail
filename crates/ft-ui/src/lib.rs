//! ft-ui — local HTTP/JSON server hosting the firetrail web UI.
//!
//! This crate is an adapter over [`ft_ops`]. It binds an axum app on
//! `127.0.0.1:0`, authenticates the local browser via a single-use
//! bootstrap token + signed `SameSite=Strict` session cookie, multiplexes
//! the [`ft_ops::EventBus`] onto an SSE stream, and exits when the SPA
//! stops heartbeating.
//!
//! Everything that is not transport plumbing lives in [`ft_ops`].
//!
//! See `docs/plans/2026-05-28-firetrail-gui-design.md` for the full design.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod assets;
pub mod auth;
pub mod error;
pub mod routes;
pub mod server;
pub mod sse;
