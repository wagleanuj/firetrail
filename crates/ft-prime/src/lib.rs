//! # ft-prime
//!
//! Context-pack generation. Builds bounded, ranked record bundles suitable
//! for priming downstream agents within a fixed context budget.
//!
//! `ft-prime` is a **pure text-processing crate**. It contains no ML, no
//! embeddings, and no I/O of its own — it consumes a [`Storage`] and an
//! [`Index`] passed by the caller and produces a deterministic
//! [`ContextPack`]. Output formatters ([`render_markdown`], [`render_json`])
//! turn that pack into agent-ready text.
//!
//! ## Relevant ADRs
//!
//! - ADR-0012 — Skill as agent docs
//! - ADR-0019 — Prime context budget (the priority order, omitted manifest,
//!   and token-estimation rules implemented here)

mod error;
mod estimate;
mod options;
mod pack;
mod render;
mod score;
mod select;

pub use error::PrimeError;
pub use estimate::estimate_tokens;
pub use options::{PrimeFormat, PrimeOptions};
pub use pack::{ContextPack, OmittedEntry, OmittedReason, PackItem};
pub use render::{render_json, render_markdown};
pub use select::{prime_for_query, prime_for_task};
