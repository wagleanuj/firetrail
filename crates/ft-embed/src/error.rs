//! Error types for the embed pipeline.

use crate::cache::CacheError;

/// Errors returned by [`crate::Embedder`] implementations and
/// [`crate::EmbedService`].
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The selected embedder backend is not available in this build
    /// (typically: ONNX requested but the `onnx` feature was not enabled, or
    /// the model file is missing).
    #[error("embedding model unavailable: {0}")]
    ModelUnavailable(String),

    /// The embedder produced a vector whose dimension differs from
    /// [`crate::Embedder::dim`].
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// Dimension advertised by the embedder.
        expected: usize,
        /// Dimension actually returned.
        actual: usize,
    },

    /// Wraps an inference-time failure from the underlying runtime.
    #[error("embedding inference failed: {0}")]
    Inference(String),

    /// Wraps cache I/O errors that bubble up through [`crate::EmbedService`].
    #[error(transparent)]
    Cache(#[from] CacheError),

    /// Wraps an I/O failure (used by the daemon transport).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Failure decoding a wire payload (daemon transport).
    #[error("protocol error: {0}")]
    Protocol(String),
}
