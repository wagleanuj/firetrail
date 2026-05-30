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
pub mod config;
pub mod daemon;
pub mod download;
pub mod embedder;
pub mod error;
#[cfg(feature = "onnx")]
pub mod onnx;
pub mod service;

pub use cache::{
    CacheError, EmbeddingCache, IntegrityIssue, IntegrityReport, SampleRow, repo_cache_dir,
    repo_cache_dir_under, repo_identity,
};
pub use config::{BuiltEmbedder, EmbeddingsConfig, Fallback, Provider, build_embedder};
pub use daemon::{
    DaemonLock, DaemonStatus, EmbedRequest, EmbedResponse, RecordIndexer, ServeOptions,
};
pub use download::{
    Artifact, ArtifactOutcome, BGE_SMALL_EN_V15_ARTIFACTS, DownloadReport, default_model_dir,
    download_artifacts, download_bge_small,
};
pub use embedder::{Embedder, MockEmbedder, OnnxEmbedder};
pub use error::EmbedError;
pub use service::{
    DocFreshness, DriftIssue, DriftReport, EmbedService, content_hash, cosine, doc_freshness,
    record_text, record_text_with_root,
};
