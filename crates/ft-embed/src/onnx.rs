//! ONNX-backed embedder for `bge-small-en-v1.5` (and compatible BERT-style
//! sentence-transformer models).
//!
//! This module is compiled only when the `onnx` cargo feature is enabled.
//! Without the feature, [`crate::embedder::OnnxEmbedder`] still exists as a
//! stub that returns [`crate::EmbedError::ModelUnavailable`] from every
//! constructor — see `embedder.rs`.
//!
//! ## Inference pipeline
//!
//! 1. Tokenise `text` with the model's `tokenizer.json` (BERT `WordPiece`).
//! 2. Build `input_ids`, `attention_mask`, `token_type_ids` as `i64`
//!    tensors of shape `[1, seq_len]`. Sequences longer than the model's
//!    max position (default `512`) are truncated by the tokenizer.
//! 3. Run the ONNX `Session` and extract `last_hidden_state` (shape
//!    `[1, seq_len, hidden_dim]`).
//! 4. Mean-pool over `seq_len` masked by `attention_mask`.
//! 5. L2-normalise.
//!
//! ## Verification
//!
//! Default cargo builds compile-test this module but cannot exercise it
//! end-to-end without a real ~33 MiB ONNX model file. The integration test
//! `onnx_bge_small_round_trips` (in `tests/onnx_bge.rs`) is gated on the
//! `FIRETRAIL_BGE_MODEL_DIR` env var and runs only when that directory
//! contains both `model.onnx` and `tokenizer.json`.

#![cfg(feature = "onnx")]

use std::path::Path;
use std::sync::Mutex;

use ort::session::{Session, builder::GraphOptimizationLevel};
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::embedder::Embedder;
use crate::error::EmbedError;

/// Canonical model id used in cache rows when loading
/// `BAAI/bge-small-en-v1.5`. Bumping the underlying weights bumps the
/// version (see [`OnnxBackend::model_version`]).
pub const BGE_SMALL_EN_V15_ID: &str = "bge-small-en-v1.5";
pub(crate) const BGE_SMALL_EN_V15_DIM: usize = 384;

/// Hidden state of an ONNX-loaded embedder. Held by
/// [`crate::embedder::OnnxEmbedder`] when the `onnx` feature is on.
pub(crate) struct OnnxBackend {
    // `Session::run` requires `&mut self`; the `Embedder` trait gives us
    // `&self`. Serialise inference through a mutex — model evaluations are
    // already CPU-bound and the daemon path is single-threaded by design
    // (see ADR-0007). The mutex is held only for the duration of one
    // inference call.
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    model_id: String,
    model_version: String,
    dim: usize,
}

impl std::fmt::Debug for OnnxBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnnxBackend")
            .field("model_id", &self.model_id)
            .field("model_version", &self.model_version)
            .field("dim", &self.dim)
            .finish_non_exhaustive()
    }
}

impl OnnxBackend {
    /// Load `model.onnx` + `tokenizer.json` from `model_dir`.
    pub(crate) fn load_from_dir(
        model_dir: &Path,
        model_id: impl Into<String>,
        model_version: impl Into<String>,
        dim: usize,
    ) -> Result<Self, EmbedError> {
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");
        if !model_path.is_file() {
            return Err(EmbedError::ModelUnavailable(format!(
                "model file not found at {}",
                model_path.display()
            )));
        }
        if !tokenizer_path.is_file() {
            return Err(EmbedError::ModelUnavailable(format!(
                "tokenizer.json not found at {}",
                tokenizer_path.display()
            )));
        }

        let session = Session::builder()
            .map_err(|e| EmbedError::ModelUnavailable(format!("ort builder: {e}")))?
            .with_optimization_level(GraphOptimizationLevel::Level1)
            .map_err(|e| EmbedError::ModelUnavailable(format!("ort opt level: {e}")))?
            .commit_from_file(&model_path)
            .map_err(|e| EmbedError::ModelUnavailable(format!("ort load model: {e}")))?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbedError::ModelUnavailable(format!("load tokenizer.json: {e}")))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            model_id: model_id.into(),
            model_version: model_version.into(),
            dim,
        })
    }

    pub(crate) fn model_id(&self) -> &str {
        &self.model_id
    }
    pub(crate) fn model_version(&self) -> &str {
        &self.model_version
    }
    pub(crate) fn dim(&self) -> usize {
        self.dim
    }

    /// Tokenise → run session → mean-pool → L2-normalise.
    pub(crate) fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| EmbedError::Inference(format!("tokenise: {e}")))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| i64::from(x)).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| i64::from(x))
            .collect();
        let type_ids: Vec<i64> = encoding
            .get_type_ids()
            .iter()
            .map(|&x| i64::from(x))
            .collect();
        let seq_len = ids.len();
        if seq_len == 0 {
            return Err(EmbedError::Inference("empty token sequence".into()));
        }

        // bge-small-en-v1.5 expects rank-2 `[1, seq_len]` integer tensors.
        // ort 2.0's `(shape, vec)` form avoids pulling ndarray into the
        // dependency graph.
        #[allow(clippy::cast_possible_wrap)]
        let shape = [1_i64, seq_len as i64];

        let mask_for_pool = mask.clone();

        let inputs = ort::inputs![
            "input_ids" => Tensor::<i64>::from_array((shape, ids))
                .map_err(|e| EmbedError::Inference(format!("tensor input_ids: {e}")))?,
            "attention_mask" => Tensor::<i64>::from_array((shape, mask))
                .map_err(|e| EmbedError::Inference(format!("tensor attention_mask: {e}")))?,
            "token_type_ids" => Tensor::<i64>::from_array((shape, type_ids))
                .map_err(|e| EmbedError::Inference(format!("tensor token_type_ids: {e}")))?,
        ];

        let mut session = self
            .session
            .lock()
            .map_err(|e| EmbedError::Inference(format!("session mutex poisoned: {e}")))?;
        let outputs = session
            .run(inputs)
            .map_err(|e| EmbedError::Inference(format!("session.run: {e}")))?;

        let last_hidden = outputs
            .get("last_hidden_state")
            .ok_or_else(|| EmbedError::Inference("missing last_hidden_state output".into()))?;
        let (out_shape, data) = last_hidden
            .try_extract_tensor::<f32>()
            .map_err(|e| EmbedError::Inference(format!("extract last_hidden_state: {e}")))?;

        // Expected shape: [1, seq_len, hidden_dim].
        if out_shape.len() != 3 || out_shape[0] != 1 {
            return Err(EmbedError::Inference(format!(
                "unexpected last_hidden_state shape {:?}; want [1, T, D]",
                &out_shape[..]
            )));
        }
        let t = usize::try_from(out_shape[1])
            .map_err(|_| EmbedError::Inference(format!("seq_len {} out of range", out_shape[1])))?;
        let d = usize::try_from(out_shape[2]).map_err(|_| {
            EmbedError::Inference(format!("hidden_dim {} out of range", out_shape[2]))
        })?;
        if d != self.dim {
            return Err(EmbedError::DimensionMismatch {
                expected: self.dim,
                actual: d,
            });
        }
        if t != seq_len {
            return Err(EmbedError::Inference(format!(
                "model returned seq_len={t} but tokenizer produced {seq_len}"
            )));
        }

        // Mean-pool over T weighted by attention_mask.
        let mut pooled = vec![0.0_f32; d];
        let mut denom = 0.0_f32;
        for (ti, &mi) in mask_for_pool.iter().enumerate() {
            if mi == 0 {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let m = mi as f32;
            denom += m;
            let base = ti * d;
            for (di, p) in pooled.iter_mut().enumerate() {
                *p += data[base + di] * m;
            }
        }
        if denom == 0.0 {
            return Err(EmbedError::Inference(
                "attention_mask sums to zero; nothing to pool".into(),
            ));
        }
        for p in &mut pooled {
            *p /= denom;
        }

        // L2-normalise. The bge-* family is trained for cosine similarity;
        // the canonical sentence-transformers pipeline always normalises
        // the pooled output.
        l2_normalize(&mut pooled);
        Ok(pooled)
    }
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Erased-trait helper. Avoids exposing `OnnxBackend` as `pub` outside the
/// crate while letting `embedder.rs` hold one.
impl Embedder for OnnxBackend {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        OnnxBackend::embed(self, text)
    }
    fn dim(&self) -> usize {
        OnnxBackend::dim(self)
    }
    fn model_id(&self) -> &str {
        OnnxBackend::model_id(self)
    }
    fn model_version(&self) -> &str {
        OnnxBackend::model_version(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_normalize_makes_unit_vector() {
        let mut v = vec![3.0_f32, 4.0];
        l2_normalize(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "norm = {norm}");
    }

    #[test]
    fn l2_normalize_handles_zero_vector() {
        let mut v = vec![0.0_f32; 4];
        l2_normalize(&mut v);
        assert!(v.iter().all(|&x| x == 0.0));
    }
}
