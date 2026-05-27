//! # ft-embed
//!
//! Local embedding pipeline: a pluggable [`Embedder`] trait, a content-hash
//! keyed [`EmbeddingCache`], and an [`EmbedService`] that combines the two.
//!
//! The full ADR-0007 daemon (single-process queue, lifecycle management,
//! multi-tenant throttling) is **out of scope for M3**. This crate ships:
//!
//! - [`Embedder`] trait + [`MockEmbedder`] (deterministic, dependency-free).
//! - An ONNX-backed embedder behind the `onnx` cargo feature (uses `ort` +
//!   `bge-small-en-v1.5`). Disabled by default to keep CI hermetic.
//! - [`EmbeddingCache`] — SQLite-backed, keyed by `(model_id, content_hash)`,
//!   each row protected by a BLAKE3 integrity checksum.
//! - [`EmbedService`] — combines an [`Embedder`] with an [`EmbeddingCache`]
//!   and gives [`ft_core::Record`]-aware helpers.
//! - A minimal [`daemon`] module: status / send / serve over a Unix-domain
//!   socket using length-prefixed JSON framing.
//!
//! ## Relevant ADRs
//!
//! - ADR-0005 — No LLM in the tool
//! - ADR-0007 — Local embeddings daemon
//!
//! ## Enabling ONNX
//!
//! ```text
//! cargo build -p ft-embed --features onnx
//! ```
//!
//! The model file path is supplied at runtime via [`OnnxEmbedder::load`].
//! Default builds (no feature) compile [`OnnxEmbedder`] as a stub that
//! returns [`EmbedError::ModelUnavailable`].

pub mod cache;
pub mod daemon;
pub mod embedder;
pub mod error;
pub mod service;

pub use cache::{CacheError, EmbeddingCache, IntegrityIssue, IntegrityReport};
pub use daemon::{DaemonStatus, EmbedRequest, EmbedResponse};
pub use embedder::{Embedder, MockEmbedder, OnnxEmbedder};
pub use error::EmbedError;
pub use service::{EmbedService, content_hash, record_text};
