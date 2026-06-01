//! tract-backed embedder for `bge-small-en-v1.5` (and compatible BERT-style
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
//! 3. Run the tract runnable plan and extract `last_hidden_state` (shape
//!    `[1, seq_len, hidden_dim]`).
//! 4. Mean-pool over `seq_len` masked by `attention_mask`.
//! 5. L2-normalise.
//!
//! Inference is pure Rust (`tract-onnx`): there is no native ONNX Runtime
//! library and no platform-specific prebuilt binary, so the build links on
//! every target.
//!
//! ## Verification
//!
//! Default cargo builds compile-test this module but cannot exercise it
//! end-to-end without a real ~33 MiB ONNX model file. The integration test
//! `onnx_bge_small_round_trips` (in `tests/onnx_bge.rs`) is gated on the
//! `onnx` feature + the LFS-bundled model and runs only with `--ignored`.

#![cfg(feature = "onnx")]

use std::path::Path;

use tokenizers::Tokenizer;
use tract_onnx::prelude::*;

use crate::embedder::Embedder;
use crate::error::EmbedError;

/// Canonical model id used in cache rows when loading
/// `BAAI/bge-small-en-v1.5`. Bumping the underlying weights bumps the
/// version (see [`OnnxBackend::model_version`]).
pub const BGE_SMALL_EN_V15_ID: &str = "bge-small-en-v1.5";
pub(crate) const BGE_SMALL_EN_V15_DIM: usize = 384;

/// The three BERT inputs, in the order tract receives them positionally.
/// `set_input_fact` / `run` use this index order, so input construction must
/// match it exactly.
const INPUT_ORDER: [&str; 3] = ["input_ids", "attention_mask", "token_type_ids"];

/// Symbolic dimension names this ONNX export uses for its inputs. tract bakes
/// these symbols into intermediate ops (e.g. the `Unsqueeze` that builds the
/// extended attention mask asserts an output shape of
/// `batch_size,1,sequence_length`). We must declare the input facts with the
/// *same* symbols, or `into_optimized()` fails trying to unify a fresh symbol
/// (or a concrete dim) against `batch_size`/`sequence_length`.
const SYM_BATCH: &str = "batch_size";
const SYM_SEQ: &str = "sequence_length";

/// A loaded, optimized tract execution plan. `into_runnable()` returns the
/// plan wrapped in an `Arc` (tract's `run` is defined on `&Arc<Self>`), so the
/// alias keeps the `Arc`.
type Plan = std::sync::Arc<TypedRunnableModel>;

/// Hidden state of a tract-loaded embedder. Held by
/// [`crate::embedder::OnnxEmbedder`] when the `onnx` feature is on.
pub(crate) struct OnnxBackend {
    model: Plan,
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

        // Load the ONNX graph with type/shape inference.
        let mut infer = tract_onnx::onnx()
            .model_for_path(&model_path)
            .map_err(|e| EmbedError::ModelUnavailable(format!("tract load model: {e}")))?;

        // bge-small accepts a variable batch and sequence length. Declare each
        // input as i64 `[batch_size, sequence_length]` reusing the export's own
        // symbol names (see `SYM_BATCH`/`SYM_SEQ`) so the graph is optimized
        // once and reused for any sequence length. Using a fresh symbol or a
        // concrete dim here makes `into_optimized()` fail to unify against the
        // symbols already baked into the graph's Unsqueeze/AddDims nodes.
        let batch = infer.symbols.sym(SYM_BATCH);
        let seq = infer.symbols.sym(SYM_SEQ);
        for (i, name) in INPUT_ORDER.iter().enumerate() {
            infer
                .set_input_fact(
                    i,
                    InferenceFact::dt_shape(i64::datum_type(), tvec![batch.to_dim(), seq.to_dim()]),
                )
                .map_err(|e| {
                    EmbedError::ModelUnavailable(format!("tract input fact for {name}: {e}"))
                })?;
        }

        let model = infer
            .into_optimized()
            .map_err(|e| EmbedError::ModelUnavailable(format!("tract optimize: {e}")))?
            .into_runnable()
            .map_err(|e| EmbedError::ModelUnavailable(format!("tract runnable: {e}")))?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbedError::ModelUnavailable(format!("load tokenizer.json: {e}")))?;

        Ok(Self {
            model,
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

    /// Tokenise → run plan → mean-pool → L2-normalise.
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

        let mask_for_pool = mask.clone();

        // Build the three rank-2 `[1, seq_len]` i64 input tensors in
        // `INPUT_ORDER`. `tract_ndarray` is re-exported by tract-onnx, so no
        // separate ndarray dependency is needed.
        let to_tensor = |v: Vec<i64>| -> Result<Tensor, EmbedError> {
            tract_ndarray::Array2::from_shape_vec((1, seq_len), v)
                .map(Tensor::from)
                .map_err(|e| EmbedError::Inference(format!("build input tensor: {e}")))
        };
        let inputs = tvec!(
            to_tensor(ids)?.into(),
            to_tensor(mask)?.into(),
            to_tensor(type_ids)?.into(),
        );

        let outputs = self
            .model
            .run(inputs)
            .map_err(|e| EmbedError::Inference(format!("tract run: {e}")))?;

        // last_hidden_state is the first model output: shape [1, seq_len, D].
        // `outputs[0]` is a `TValue`; deref reaches the underlying `Tensor`,
        // which exposes `to_array_view`.
        let view = (*outputs[0])
            .to_plain_array_view::<f32>()
            .map_err(|e| EmbedError::Inference(format!("extract last_hidden_state: {e}")))?;
        let out_shape = view.shape();
        let data = view
            .as_slice()
            .ok_or_else(|| EmbedError::Inference("last_hidden_state is not contiguous".into()))?;

        // Expected shape: [1, seq_len, hidden_dim].
        if out_shape.len() != 3 || out_shape[0] != 1 {
            return Err(EmbedError::Inference(format!(
                "unexpected last_hidden_state shape {out_shape:?}; want [1, T, D]"
            )));
        }
        let t = out_shape[1];
        let d = out_shape[2];
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
