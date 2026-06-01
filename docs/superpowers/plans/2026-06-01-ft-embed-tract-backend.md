# ft-embed tract Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the native `ort` (ONNX Runtime) backend in `ft-embed` with the pure-Rust `tract-onnx` engine so real `bge-small-en-v1.5` inference works on every target (Intel macOS included), and restore `x86_64-apple-darwin` + drop the glibc≥2.39 floor in the release config.

**Architecture:** All `ort` API usage is confined to `crates/ft-embed/src/onnx.rs` (`OnnxBackend`). We swap that one struct's internals to tract: load `model.onnx` → declare symbolic-sequence input facts → optimize → runnable plan; build i64 input tensors via `tract_ndarray`; run; reuse the existing engine-agnostic mean-pool + L2-normalize code verbatim. The public `Embedder` trait, `OnnxEmbedder` type, the `onnx` feature name, and `model_version` are all unchanged, so the rest of the workspace and the existing gated integration test are untouched. Because there is no native library, the release config gains Intel macOS and loses its custom high-glibc Linux runners.

**Tech Stack:** Rust (edition 2024), `tract-onnx` 0.23, `tract_ndarray` (re-exported by tract), `tokenizers` 0.20 (unchanged), cargo-dist 0.32 for release packaging.

**Spec:** `docs/superpowers/specs/2026-06-01-ft-embed-tract-backend-design.md`
**Issue:** firetrail-huvf

---

## Background the engineer needs

- **What `OnnxBackend` does:** tokenize text (BERT WordPiece) → feed three i64 tensors `input_ids`, `attention_mask`, `token_type_ids` of shape `[1, seq_len]` → run the ONNX graph → take the `last_hidden_state` output `[1, seq_len, 384]` → mean-pool over the sequence weighted by `attention_mask` → L2-normalize → return `Vec<f32>` of length 384. Only the load + run mechanics change; the tokenize/pool/normalize logic is identical.
- **The model is on disk via Git LFS** at `crates/ft-embed/models/bge-small-en-v1.5/` (`model.onnx` + `tokenizer.json`). If those files are LFS pointer stubs, run `git lfs pull` before the integration test. `$FIRETRAIL_BGE_MODEL_DIR` overrides the location.
- **The gate is an existing test:** `crates/ft-embed/tests/onnx_bge.rs::onnx_bge_small_round_trips`. It is `#[ignore]`d and `#[cfg(feature = "onnx")]`, and asserts: output is 384-dim, L2-normalized (‖v‖≈1), deterministic across two calls, and `cos(dog,puppy) > cos(dog,spaceship)`. **You do not write a new test** — this one drives the implementation. Run it with the model present.
- **`tract` runs on `&self`.** Unlike `ort::Session::run` (which needs `&mut self` and forced a `Mutex<Session>`), a tract runnable plan's `.run()` takes `&self`. So the `Mutex` is removed and `OnnxBackend` becomes lock-free, satisfying the `Embedder::embed(&self, …)` signature directly.
- **`tract_ndarray` is re-exported** by `tract-onnx` as `tract_onnx::prelude::tract_ndarray` — no separate `ndarray` dependency is added.
- **Workspace lints:** `unsafe_code = "forbid"` (our code stays safe — tract's internal unsafe is its own crate's concern), plus `clippy::all` + `clippy::pedantic` at warn. Keep the existing `#[allow(clippy::cast_possible_wrap)]` / `cast_precision_loss` annotations where casts remain.

---

## Task 1: Swap the dependency (`ort` → `tract-onnx`)

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`, around lines 85-88)
- Modify: `crates/ft-embed/Cargo.toml` (feature definition + optional dep, lines ~12-16 and ~35)

- [ ] **Step 1: Replace the workspace dependency**

In `Cargo.toml`, replace the ML-runtime block:

```toml
# ML runtime (ONNX), added in M3
ort = "2.0.0-rc.5"
# BERT-style tokenizer for bge-small-en-v1.5 (M3 ONNX path).
tokenizers = { version = "0.20", default-features = false, features = ["onig"] }
```

with:

```toml
# ML runtime: pure-Rust ONNX inference (Sonos tract). Replaced `ort` (native
# ONNX Runtime) in firetrail-huvf so the ONNX build links on every target,
# including x86_64-apple-darwin, with no native library or glibc floor.
tract-onnx = "0.23"
# BERT-style tokenizer for bge-small-en-v1.5 (M3 ONNX path).
tokenizers = { version = "0.20", default-features = false, features = ["onig"] }
```

- [ ] **Step 2: Update the crate's feature + optional dep**

In `crates/ft-embed/Cargo.toml`, change the feature definition:

```toml
# Enable the real ONNX-backed embedder (`bge-small-en-v1.5`) via pure-Rust tract.
onnx = ["dep:tract-onnx", "dep:tokenizers"]
```

(keep the `default = ["onnx"]` line and the leading doc comment above it as-is)

and replace the optional dependency line:

```toml
tract-onnx = { workspace = true, optional = true }
```

(delete the old `ort = { workspace = true, optional = true }` line; keep `tokenizers = { workspace = true, optional = true }`)

- [ ] **Step 3: Verify the no-onnx build still compiles (the stub path)**

This must pass even before `onnx.rs` is ported, because `--no-default-features` never touches `onnx.rs`.

Run: `cargo build -p ft-embed --no-default-features`
Expected: PASS (compiles the `MockEmbedder` + `OnnxEmbedder` stub path; no `ort`/`tract` involved).

- [ ] **Step 4: Confirm the onnx build now fails to compile (expected — `onnx.rs` still calls `ort`)**

Run: `cargo build -p ft-embed --features onnx`
Expected: FAIL with unresolved-import errors for `ort::...` in `onnx.rs`. This is the failing state Task 2 fixes.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/ft-embed/Cargo.toml Cargo.lock
git commit -m "build(ft-embed): swap ort for pure-Rust tract-onnx

Refs: firetrail-huvf"
```

---

## Task 2: Port `OnnxBackend` to tract

**Files:**
- Modify (rewrite the body): `crates/ft-embed/src/onnx.rs`
- Gate/driver test (do NOT edit): `crates/ft-embed/tests/onnx_bge.rs`

This is the core task. The entire `onnx.rs` is replaced below. The tokenize/pool/normalize/validation logic is preserved; only imports, the struct fields, `load_from_dir`, and the run/extract portion of `embed` change. The `l2_normalize` helper and its two unit tests are kept verbatim. The `Embedder` impl is kept verbatim.

- [ ] **Step 1: Ensure the LFS model is present (prerequisite for the gate test)**

Run: `git lfs pull && ls -lh crates/ft-embed/models/bge-small-en-v1.5/`
Expected: `model.onnx` (~33 MiB) and `tokenizer.json` are real files, not ~130-byte pointer stubs. If they are stubs, the integration test cannot run.

- [ ] **Step 2: Rewrite `crates/ft-embed/src/onnx.rs`**

Replace the entire file with:

```rust
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

/// A loaded, optimized tract execution plan.
type Plan = TypedRunnableModel<TypedModel>;

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

        // bge-small accepts a variable sequence length. Declare each input as
        // i64 `[1, S]` with a single symbolic dimension `S` so the graph is
        // optimized once and reused for any sequence length.
        let s = infer.symbol_table.sym("S");
        for (i, name) in INPUT_ORDER.iter().enumerate() {
            infer
                .set_input_fact(
                    i,
                    InferenceFact::dt_shape(i64::datum_type(), tvec![1.to_dim(), s.to_dim()]),
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
        let view = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| EmbedError::Inference(format!("extract last_hidden_state: {e}")))?;
        let out_shape = view.shape();
        let data = view.as_slice().ok_or_else(|| {
            EmbedError::Inference("last_hidden_state is not contiguous".into())
        })?;

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
```

- [ ] **Step 3: Compile the onnx build**

Run: `cargo build -p ft-embed --features onnx`
Expected: PASS.

**If it fails on a tract API name** (the three version-sensitive spots are: the `Plan` type alias `TypedRunnableModel<TypedModel>`, the `infer.symbol_table` field, and `InferenceFact::dt_shape(...)`), resolve compiler-driven — they are all in `load_from_dir` and the type alias:
  - Type alias: it is exactly the return type of `.into_optimized()?.into_runnable()?`. If the compiler reports a different concrete type, set `type Plan = …` to what it prints.
  - `symbol_table`: if the field is named differently (e.g. `symbols`), use that; the method is `.sym("S")`.
  - `dt_shape`: if unavailable, build the fact as `i64::fact(&[1.to_dim(), s.to_dim()]).into()` and pass that to `set_input_fact`.

- [ ] **Step 4: Run the cheap unit tests (no model needed)**

Run: `cargo test -p ft-embed --features onnx -- l2_normalize`
Expected: PASS (`l2_normalize_makes_unit_vector`, `l2_normalize_handles_zero_vector`).

- [ ] **Step 5: Run the gated integration test against the real model**

Run: `cargo test -p ft-embed --features onnx --test onnx_bge -- --ignored --nocapture`
Expected: PASS — `onnx_bge_small_round_trips` confirms 384-dim, unit-norm, deterministic, and `cos(dog,puppy) > cos(dog,spaceship)`.

**If `into_optimized()` fails on a dynamic/symbolic-shape op (e.g. `ConstantOfShape` with a symbolic input):** fall back to a fixed-length plan. Pad each input to 512 tokens (the bge max position) and fix the input facts to a concrete `[1, 512]`:
  - In `load_from_dir`, replace `tvec![1.to_dim(), s.to_dim()]` with `tvec![1.to_dim(), 512.to_dim()]` and drop the `let s = …` line.
  - In `embed`, after building `ids`/`mask`/`type_ids`, pad each to length 512 (`ids` and `type_ids` with `0`, `mask` with `0`) and set `let seq_len = 512;` *before* tensor construction; the zero `attention_mask` entries make the mean-pool ignore the padding, so the output is unchanged. The model's `[1,512,384]` output then satisfies the existing `t == seq_len` check.
  This is strictly more robust (one fixed shape, fully optimized) at the cost of always running 512 tokens. Use it only if symbolic optimization fails.

- [ ] **Step 6: Commit**

```bash
git add crates/ft-embed/src/onnx.rs
git commit -m "feat(ft-embed): run bge-small inference through tract

Port OnnxBackend from ort to pure-Rust tract-onnx. Load -> symbolic-seq
input facts -> optimize -> runnable plan; build i64 inputs via tract_ndarray;
reuse the existing mean-pool + L2-normalize path. Drops the Session mutex
(tract runs on &self).

Refs: firetrail-huvf"
```

---

## Task 3: Update stale `ort` mentions in doc comments

**Files:**
- Modify: `crates/ft-embed/src/embedder.rs:152-156` (doc comment on `OnnxEmbedder`)
- Modify: `crates/ft-embed/src/lib.rs:10` (crate-level doc)

No code/behavior change — only comments that still name `ort`.

- [ ] **Step 1: Fix the `OnnxEmbedder` doc comment**

In `crates/ft-embed/src/embedder.rs`, find:

```rust
/// With feature `onnx` **on**, [`OnnxEmbedder::load_dir`] initialises an
/// `ort::Session` plus a `tokenizer.json` from a model directory. Inference
/// runs BERT-style tokenisation, executes the ONNX graph, mean-pools over
/// the last hidden state weighted by the attention mask, and L2-normalises
/// the result. See [`crate::onnx`] for the pipeline.
```

replace with:

```rust
/// With feature `onnx` **on**, [`OnnxEmbedder::load_dir`] initialises a
/// pure-Rust `tract` runnable plan plus a `tokenizer.json` from a model
/// directory. Inference runs BERT-style tokenisation, executes the ONNX
/// graph, mean-pools over the last hidden state weighted by the attention
/// mask, and L2-normalises the result. See [`crate::onnx`] for the pipeline.
```

- [ ] **Step 2: Fix the crate-level doc**

In `crates/ft-embed/src/lib.rs`, find the line (around line 10):

```rust
//! - An ONNX-backed embedder behind the `onnx` cargo feature (uses `ort` +
```

replace `ort` with `tract`:

```rust
//! - An ONNX-backed embedder behind the `onnx` cargo feature (uses `tract` +
```

(if the sentence continues on the next line mentioning `tokenizers`, leave that part intact)

- [ ] **Step 3: Verify docs build cleanly**

Run: `cargo doc -p ft-embed --features onnx --no-deps`
Expected: PASS, no broken-intra-doc-link warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/ft-embed/src/embedder.rs crates/ft-embed/src/lib.rs
git commit -m "docs(ft-embed): refer to tract backend instead of ort

Refs: firetrail-huvf"
```

---

## Task 4: Restore Intel macOS + drop the glibc floor in the release config

**Files:**
- Modify: `dist-workspace.toml`
- Regenerate (do NOT hand-edit): `.github/workflows/release.yml`

- [ ] **Step 1: Add the Intel-mac target and refresh its comment**

In `dist-workspace.toml`, replace:

```toml
# Target platforms to build apps for (Rust target-triple syntax).
# x86_64-apple-darwin is intentionally excluded: `ort` (ONNX runtime) ships no
# prebuilt binary for Intel macOS, so the ONNX-enabled build cannot link there.
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
```

with:

```toml
# Target platforms to build apps for (Rust target-triple syntax).
# x86_64-apple-darwin is back: the embedding backend is pure-Rust tract
# (firetrail-huvf), so the ONNX-enabled build links on every target with no
# platform-specific prebuilt runtime.
targets = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]
```

- [ ] **Step 2: Remove the high-glibc Linux runner overrides**

In `dist-workspace.toml`, delete the entire custom-runners block (the comment + the `[dist.github-custom-runners]` table):

```toml
# The prebuilt onnxruntime that `ort` links references glibc >= 2.38 symbols
# (__isoc23_strtoull etc.), which ubuntu-22.04 (glibc 2.35) lacks. Build the
# Linux targets on ubuntu-24.04 (glibc 2.39) so ONNX links.
[dist.github-custom-runners]
x86_64-unknown-linux-gnu = "ubuntu-24.04"
aarch64-unknown-linux-gnu = "ubuntu-24.04-arm"
```

With tract there is no native onnxruntime, so the glibc≥2.39 symbol requirement is gone and dist's default (lower-glibc) Linux runners are correct. Leave the rest of the file (installers, `github-build-setup`, hosting, etc.) unchanged.

- [ ] **Step 3: Regenerate the release workflow**

The `.github/workflows/release.yml` is autogenerated by cargo-dist and must never be hand-edited. Regenerate it from the edited config.

Run: `dist generate`
Expected: `.github/workflows/release.yml` is rewritten. If `dist` is not installed locally, install the pinned version first:
`curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v0.32.0/cargo-dist-installer.sh | sh`

- [ ] **Step 4: Confirm the regen reflects the new matrix**

Run: `git diff .github/workflows/release.yml | grep -iE "x86_64-apple-darwin|ubuntu-24.04|macos-13"`
Expected: the diff shows `x86_64-apple-darwin` added to the build matrix and the `ubuntu-24.04` runner overrides removed (the Linux jobs revert to dist's default runner). It will pick a macOS runner for the Intel target; that is expected.

- [ ] **Step 5: Sanity-check the plan output**

Run: `dist plan`
Expected: PASS, listing four build targets including `x86_64-apple-darwin`, with no errors.

- [ ] **Step 6: Commit**

```bash
git add dist-workspace.toml .github/workflows/release.yml
git commit -m "ci(release): restore x86_64-apple-darwin, drop glibc>=2.39 floor

tract removes the native onnxruntime dependency, so Intel macOS links again
and Linux no longer needs the ubuntu-24.04 (glibc 2.39) runners.

Refs: firetrail-huvf"
```

---

## Task 5: Full-workspace verification

**Files:** none (verification only)

- [ ] **Step 1: Confirm `ort` is gone from the dependency graph**

Run: `cargo tree -p ft-embed --features onnx -i ort 2>&1 | head; echo "---"; grep -c 'name = "ort"' Cargo.lock`
Expected: `cargo tree` reports `ort` is not in the graph (errors with "package ID specification `ort` did not match any packages"); the `grep -c` prints `0`. `cargo tree -p ft-embed --features onnx -i tract-onnx` should instead show tract present.

- [ ] **Step 2: Build the whole workspace on default features**

Run: `cargo build --workspace`
Expected: PASS (default features include `onnx`).

- [ ] **Step 3: Test the whole workspace**

Run: `cargo test --workspace`
Expected: PASS. (The `#[ignore]`d `onnx_bge_small_round_trips` is skipped here; it was already exercised in Task 2 Step 5.)

- [ ] **Step 4: Clippy clean (lints are warn; treat warnings as failures)**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings. (`unsafe_code = "forbid"` and `clippy::pedantic` both hold.)

- [ ] **Step 5: Format check**

Run: `cargo fmt --all -- --check`
Expected: PASS (no diff).

- [ ] **Step 6: Prove the Intel-mac target compiles (if the toolchain target is available)**

Run: `rustup target add x86_64-apple-darwin && cargo build -p ft-cli --target x86_64-apple-darwin`
Expected: PASS — this is the regression the whole task exists to fix (the old `ort` build failed to link here). If `ft-cli` is not the ONNX-bearing binary, build whichever crate enables `ft-embed`'s `onnx` feature (e.g. `ft-ui`); the point is to compile an `onnx`-enabled binary for `x86_64-apple-darwin`. If building on a non-mac host without the cross-linker, note it and rely on the release CI matrix (Task 4) as the proof instead.

- [ ] **Step 7: Final state check (no commit — verification only)**

Run: `git status`
Expected: clean working tree; all changes already committed across Tasks 1-4.

---

## Self-review notes (author)

- **Spec coverage:** dependency swap (Task 1), onnx.rs port incl. mutex removal + reused pooling (Task 2), naming kept / API unchanged (no rename tasks, by design), doc-comment touch-ups (Task 3), Intel target + glibc floor + dist regen (Task 4), build/test/clippy + Intel-target proof + `ort`-absence check (Task 5). No vector-parity gate (spec: nothing indexed) — correctly absent.
- **Type consistency:** `OnnxBackend` field `model: Plan`, `Plan = TypedRunnableModel<TypedModel>`; `INPUT_ORDER` drives both `set_input_fact` indices and the `tvec!` input order; `embed` reuses `l2_normalize` (defined in same file) and `EmbedError::{Inference, DimensionMismatch, ModelUnavailable}` (existing variants, confirmed in `error.rs` usage). Public surface (`OnnxEmbedder::load_dir`/`load_bge_small`, `Embedder`, `cosine`) untouched, so `tests/onnx_bge.rs` compiles unchanged.
- **Version-sensitive API:** three spots in Task 2 (type alias, `symbol_table`, `dt_shape`) carry explicit compiler-driven resolution notes, plus a documented fixed-512 fallback if symbolic optimization fails — driven by the existing integration test.
```
