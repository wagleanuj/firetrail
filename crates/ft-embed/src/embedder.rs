//! [`Embedder`] trait + concrete implementations.
//!
//! Two implementations are provided:
//!
//! - [`MockEmbedder`] — deterministic, dependency-free, used by tests and as a
//!   fallback when no real model is available. Complements (does not replace)
//!   the simpler `MockEmbedder` in `ft-testkit`, which exists only for
//!   compile-time ranking-test scaffolding.
//! - [`OnnxEmbedder`] — feature-gated on `onnx`. When the feature is off this
//!   type still exists, but every constructor returns
//!   [`crate::EmbedError::ModelUnavailable`] so downstream code can compile
//!   uniformly.

use crate::error::EmbedError;

/// Blanket impl so callers can hold a heap-allocated trait object
/// (`Box<dyn Embedder>`) wherever the API expects an `E: Embedder`. Used by
/// [`crate::config::build_embedder`] / [`crate::EmbedService`] when the
/// concrete embedder type isn't known until config-resolution time.
impl<E: Embedder + ?Sized> Embedder for Box<E> {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        (**self).embed(text)
    }
    fn dim(&self) -> usize {
        (**self).dim()
    }
    fn model_id(&self) -> &str {
        (**self).model_id()
    }
    fn model_version(&self) -> &str {
        (**self).model_version()
    }
}

/// Synchronous embedding interface.
///
/// Implementations must be deterministic for a given `(model_id, text)` pair
/// so that the content-hash cache is meaningful.
pub trait Embedder: Send + Sync {
    /// Embed `text` into a vector of length [`Self::dim`].
    ///
    /// # Errors
    /// Returns [`EmbedError::ModelUnavailable`] if the backend can't run, or
    /// [`EmbedError::Inference`] for runtime failures.
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError>;

    /// Dimensionality of the embedding vector.
    fn dim(&self) -> usize;

    /// Stable identifier (e.g. `"bge-small-en-v1.5"` or
    /// `"mock-384-seed42"`) used as a cache partition key.
    fn model_id(&self) -> &str;

    /// Stable version tag for the underlying model weights. Cache rows are
    /// partitioned by `(model_id, model_version, content_hash)`, so bumping
    /// the version cleanly invalidates prior entries without colliding with
    /// new ones (ADR-0007 §"Integrity verification"). Defaults to `"1"`.
    ///
    /// The return type is tied to `&self` so concrete implementations may
    /// return owned-string fields (mirrors [`Self::model_id`]).
    #[allow(clippy::unnecessary_literal_bound)]
    fn model_version(&self) -> &str {
        "1"
    }
}

/// Deterministic, dependency-free embedder used by tests and as a degraded
/// fallback when no ONNX model is installed.
///
/// The output is a unit vector derived from the BLAKE3 hash of `text` mixed
/// with `seed`. Two calls with the same `(seed, dim, text)` always produce
/// the same vector, which keeps the content-hash cache meaningful in tests.
#[derive(Debug, Clone)]
pub struct MockEmbedder {
    seed: u64,
    dim: usize,
    model_id: String,
}

impl MockEmbedder {
    /// Construct a [`MockEmbedder`] with the given seed and output dim.
    #[must_use]
    pub fn new(seed: u64, dim: usize) -> Self {
        Self {
            seed,
            dim,
            model_id: format!("mock-{dim}-seed{seed}"),
        }
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new(0, 384)
    }
}

impl Embedder for MockEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        // Derive `dim` f32 values from a BLAKE3 XOF keyed on (seed || text).
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.seed.to_le_bytes());
        hasher.update(text.as_bytes());
        let mut xof = hasher.finalize_xof();

        let mut out = Vec::with_capacity(self.dim);
        // Read 4 bytes per element.
        let mut buf = [0u8; 4];
        for _ in 0..self.dim {
            xof.fill(&mut buf);
            // Map u32 → [-1.0, 1.0] without using unsafe / float casts that
            // clippy::cast_precision_loss complains about.
            let v = u32::from_le_bytes(buf);
            // Normalise into [0, 1) via division, then shift to [-1, 1).
            #[allow(clippy::cast_precision_loss)]
            let f = f64::from(v) / f64::from(u32::MAX);
            #[allow(clippy::cast_possible_truncation)]
            let f = (f.mul_add(2.0, -1.0)) as f32;
            out.push(f);
        }

        // L2-normalise so cosine similarity behaves like real embeddings.
        let norm_sq: f32 = out.iter().map(|x| x * x).sum();
        let norm = norm_sq.sqrt();
        if norm > 0.0 {
            for x in &mut out {
                *x /= norm;
            }
        }
        Ok(out)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

// ---------------------------------------------------------------------------
// OnnxEmbedder — feature-gated on `onnx`.
// ---------------------------------------------------------------------------

/// ONNX-backed embedder (default: `bge-small-en-v1.5`).
///
/// With feature `onnx` **off**, this type still compiles, but every
/// constructor returns [`EmbedError::ModelUnavailable`]. This lets dependents
/// compile against the same surface in both modes.
///
/// With feature `onnx` **on**, [`OnnxEmbedder::load_dir`] initialises an
/// `ort::Session` plus a `tokenizer.json` from a model directory. Inference
/// runs BERT-style tokenisation, executes the ONNX graph, mean-pools over
/// the last hidden state weighted by the attention mask, and L2-normalises
/// the result. See [`crate::onnx`] for the pipeline.
#[derive(Debug)]
pub struct OnnxEmbedder {
    #[cfg(feature = "onnx")]
    inner: crate::onnx::OnnxBackend,
    #[cfg(not(feature = "onnx"))]
    model_id: String,
    #[cfg(not(feature = "onnx"))]
    dim: usize,
}

impl OnnxEmbedder {
    /// Load `model.onnx` + `tokenizer.json` from `model_dir` and tag the
    /// resulting embedder with `model_id` / `model_version`.
    ///
    /// `dim` is the embedding dimensionality the model produces (384 for
    /// `bge-small-en-v1.5`). It is checked against the actual model output
    /// on every call to [`Embedder::embed`].
    ///
    /// # Errors
    /// Returns [`EmbedError::ModelUnavailable`] when the `onnx` feature is
    /// disabled, when the model or tokenizer file is missing, or when `ort`
    /// or `tokenizers` rejects the input.
    #[cfg(feature = "onnx")]
    pub fn load_dir(
        model_dir: &std::path::Path,
        model_id: impl Into<String>,
        model_version: impl Into<String>,
        dim: usize,
    ) -> Result<Self, EmbedError> {
        let inner = crate::onnx::OnnxBackend::load_from_dir(model_dir, model_id, model_version, dim)?;
        Ok(Self { inner })
    }

    /// Convenience: load `bge-small-en-v1.5` (384-dim) from `model_dir`.
    /// Equivalent to [`Self::load_dir`] with the canonical model id.
    #[cfg(feature = "onnx")]
    pub fn load_bge_small(model_dir: &std::path::Path) -> Result<Self, EmbedError> {
        Self::load_dir(model_dir, crate::onnx::BGE_SMALL_EN_V15_ID, "1", crate::onnx::BGE_SMALL_EN_V15_DIM)
    }

    /// Stub constructor used when the `onnx` feature is **disabled**.
    ///
    /// # Errors
    /// Always returns [`EmbedError::ModelUnavailable`].
    #[cfg(not(feature = "onnx"))]
    #[allow(clippy::needless_pass_by_value)]
    pub fn load_dir(
        _model_dir: &std::path::Path,
        _model_id: impl Into<String>,
        _model_version: impl Into<String>,
        _dim: usize,
    ) -> Result<Self, EmbedError> {
        Err(EmbedError::ModelUnavailable(
            "ft-embed was built without the `onnx` feature".to_string(),
        ))
    }

    /// Stub `bge-small-en-v1.5` constructor when the `onnx` feature is off.
    #[cfg(not(feature = "onnx"))]
    #[allow(clippy::needless_pass_by_value)]
    pub fn load_bge_small(_model_dir: &std::path::Path) -> Result<Self, EmbedError> {
        Err(EmbedError::ModelUnavailable(
            "ft-embed was built without the `onnx` feature".to_string(),
        ))
    }
}

impl Embedder for OnnxEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        #[cfg(feature = "onnx")]
        {
            self.inner.embed(text)
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = text;
            Err(EmbedError::ModelUnavailable(
                "ft-embed was built without the `onnx` feature".to_string(),
            ))
        }
    }

    fn dim(&self) -> usize {
        #[cfg(feature = "onnx")]
        {
            self.inner.dim()
        }
        #[cfg(not(feature = "onnx"))]
        {
            self.dim
        }
    }

    fn model_id(&self) -> &str {
        #[cfg(feature = "onnx")]
        {
            self.inner.model_id()
        }
        #[cfg(not(feature = "onnx"))]
        {
            &self.model_id
        }
    }

    fn model_version(&self) -> &str {
        #[cfg(feature = "onnx")]
        {
            self.inner.model_version()
        }
        #[cfg(not(feature = "onnx"))]
        {
            "1"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_embedder_is_deterministic() {
        let e = MockEmbedder::new(42, 16);
        let v1 = e.embed("hello world").unwrap();
        let v2 = e.embed("hello world").unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v1.len(), 16);
    }

    #[test]
    fn mock_embedder_distinguishes_inputs() {
        let e = MockEmbedder::new(42, 16);
        let v1 = e.embed("hello world").unwrap();
        let v2 = e.embed("hello firetrail").unwrap();
        assert_ne!(v1, v2);
    }

    #[test]
    fn mock_embedder_is_unit_normalised() {
        let e = MockEmbedder::new(7, 64);
        let v = e.embed("anything").unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm = {norm}");
    }

    #[test]
    fn mock_embedder_seed_matters() {
        let a = MockEmbedder::new(1, 16).embed("text").unwrap();
        let b = MockEmbedder::new(2, 16).embed("text").unwrap();
        assert_ne!(a, b);
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn onnx_embedder_unavailable_without_feature() {
        let r = OnnxEmbedder::load_dir(std::path::Path::new("/nonexistent"), "bge", "1", 384);
        assert!(matches!(r, Err(EmbedError::ModelUnavailable(_))));
    }

    #[cfg(feature = "onnx")]
    #[test]
    fn onnx_embedder_reports_missing_files() {
        // Empty dir → both files missing → ModelUnavailable.
        let dir = tempfile::tempdir().unwrap();
        let r = OnnxEmbedder::load_bge_small(dir.path());
        assert!(matches!(r, Err(EmbedError::ModelUnavailable(_))));
    }
}
