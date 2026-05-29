//! # ft-search
//!
//! Hybrid search over Firetrail records: lexical (`SQLite` FTS5) plus optional
//! vector similarity (sqlite-vec), unified by a single ranking layer.
//!
//! `ft-search` co-locates with [`ft_index`]: it opens the same `SQLite` database
//! that `ft-index` writes (`<workspace>/.firetrail/index.db`) and adds two
//! virtual tables alongside the existing relational schema:
//!
//! - `records_fts` — FTS5 index over record title + body text
//! - `records_vec` — `vec0` virtual table holding 384-d embeddings
//!   (feature-gated on `sqlite-vec`; absent in lexical-only mode)
//!
//! When the `sqlite-vec` Cargo feature is **off**, every vector entry-point is
//! a no-op (with a `tracing::warn` on attempted upserts) and the engine
//! transparently falls back to lexical search. This is the documented M3
//! fallback path — `firetrail search` always works, even on systems that lack
//! the extension binary.
//!
//! ## Hybrid ranking
//!
//! When both signals are available, the final score is a weighted sum:
//!
//! ```text
//! score = α·vector_sim + β·lexical_score + γ·trust_weight + δ·recency_weight
//! ```
//!
//! Default weights are tuned for "moderately precise, trust-aware" output
//! (see [`ranking`]); they can be revisited once we have real usage data.
//!
//! ## Relevant ADRs
//!
//! - ADR-0007 — Local embeddings daemon

#![deny(missing_docs)]

mod engine;
mod error;
mod hit;
mod kind;
mod query;
mod ranking;
mod schema;

pub use engine::{IndexDoc, SearchEngine};
pub use error::SearchError;
pub use hit::{HitMode, SearchHit};
pub use kind::{DocId, IndexKind};
pub use query::{SearchMode, SearchQuery};
pub use ranking::{ALPHA, BETA, DELTA, GAMMA, trust_weight};

/// Dimension of the embedding vectors stored in `records_vec`.
///
/// Matches `bge-small-en-v1.5`, which `ft-embed` exposes as the M3 default.
pub const EMBEDDING_DIM: usize = 384;
