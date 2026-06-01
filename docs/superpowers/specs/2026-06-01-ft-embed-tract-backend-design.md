# ft-embed: replace `ort` with pure-Rust `tract` (restore Intel macOS)

- **Date:** 2026-06-01
- **Issue:** firetrail-huvf — Restore x86_64-apple-darwin release coverage (ONNX backend)
- **Status:** approved (direction); spec under review

## Problem

Real embedding inference in `ft-embed` runs `bge-small-en-v1.5` through `ort`, the
Rust binding to Microsoft's native ONNX Runtime C++ library. `ort` resolves the
runtime via prebuilt downloads, and **pyke has permanently dropped the
`x86_64-apple-darwin` (Intel macOS) prebuilt** following upstream ONNX Runtime
and Rust changes, with a stated intent of "little to no macOS support" going
forward. Because the default build links `ort`, the ONNX-enabled binary cannot
*link* on Intel macOS at all — so `x86_64-apple-darwin` was removed from the
release targets in v0.1.0 (`dist-workspace.toml:14`).

A second, related defect: the prebuilt onnxruntime `ort` links references
glibc ≥ 2.38 symbols, forcing the Linux release builds onto `ubuntu-24.04`
(glibc 2.39) and so imposing a glibc ≥ 2.39 floor on Linux users at runtime
(`dist-workspace.toml:27-31`).

There are no production users and no embeddings indexed in the wild yet, so no
data migration or vector-parity guarantee is required.

## Decision

Replace `ort` with **`tract-onnx`** (Sonos' pure-Rust ONNX inference engine) as
the sole real-inference backend. `tract` compiles for every Rust target with no
native library, no download step, and no FFI — which:

1. Restores Intel macOS (and any other target) for the ONNX-enabled build.
2. Removes the glibc floor entirely (Linux builds can drop back to the lower-glibc
   default runner).
3. Aligns with the workspace `unsafe_code = "forbid"` posture by removing the
   only FFI dependency in the embedding path.

`tract` is chosen over the alternatives (build onnxruntime from source on an
Intel-mac CI runner; pin an old `ort` rc that still shipped Intel prebuilts)
because it is the only option that fixes *both* Intel coverage and the glibc
floor, needs no fragile/retiring CI runner, and does not block future dependency
upgrades.

### Naming: kept as-is (deliberate)

The `onnx` cargo feature, the `OnnxEmbedder` public type, the `onnx.rs` module,
and the `OnnxBackend` struct are **retained**. They remain accurate — this is
still ONNX-file inference — and renaming would churn ~40 references across
`config.rs`, `lib.rs`, `error.rs`, the daemon command, and tests for no
behavioral benefit. The public API surface (`Embedder` trait,
`OnnxEmbedder::load_dir`, `OnnxEmbedder::load_bge_small`) is unchanged.

### Default feature: unchanged, now genuinely portable

`ft-embed` keeps `default = ["onnx"]`. Real inference stays on by default — but
whereas default-on `ort` fails to *link* on unsupported targets, default-on
`tract` links cleanly everywhere. "Real inference, on by default, on all targets"
holds without exceptions for the first time.

## Scope of change

### 1. `crates/ft-embed/src/onnx.rs` (the only substantive code change)

`OnnxBackend` is the sole holder of the `ort` API. Replace its internals:

- **Load:** `ort::Session::builder()…commit_from_file` →
  `tract_onnx::onnx().model_for_path(model.onnx) → into_optimized() →
  into_runnable()`. Store the runnable plan in `OnnxBackend`.
- **Inputs:** build `input_ids`, `attention_mask`, `token_type_ids` as i64
  tensors of shape `[1, seq_len]` using tract's tensor type (via
  `tract-ndarray`) instead of `ort::inputs!` / `ort::value::Tensor`.
- **Run + extract:** run the plan, pull the `last_hidden_state` output of shape
  `[1, T, D]` as an `f32` slice.
- **Pool + normalize:** **reuse the existing mean-pool + L2-normalize code
  (current lines ~201-240) verbatim** — it already operates on the raw tensor
  buffer and is engine-agnostic, including the shape/dim/seq_len validation and
  the `l2_normalize` helper + its unit tests.
- **Concurrency:** drop the `Mutex<Session>` (current lines ~49-54, ~162-165).
  `ort::Session::run` required `&mut self`; a tract runnable plan runs on
  `&self`, so `OnnxBackend` becomes lock-free. The `Embedder` trait's `&self`
  signature is satisfied directly.
- Update the module doc comment (step 3 of the pipeline description) to say
  "tract runnable plan" instead of "ONNX `Session`".

### 2. Dependencies

- `Cargo.toml` `[workspace.dependencies]`: remove `ort = "2.0.0-rc.5"`; add
  `tract-onnx` (latest stable). Keep `tokenizers` (already pure Rust).
- `crates/ft-embed/Cargo.toml`: change the feature to
  `onnx = ["dep:tract-onnx", "dep:tokenizers"]`; replace the `ort` optional
  dependency line with `tract-onnx`.

### 3. Release / CI config (`dist-workspace.toml`)

- Add `"x86_64-apple-darwin"` to `targets`.
- Remove the `[dist.github-custom-runners]` block (the ubuntu-24.04 / 24.04-arm
  overrides) so Linux builds use dist's default lower-glibc runner — no native
  lib means no glibc ≥ 2.39 requirement.
- Update the now-stale comments at lines 11-13 (Intel exclusion rationale) and
  27-31 (glibc rationale).
- Regenerate `.github/workflows/release.yml` with `dist generate` (the workflow
  is autogenerated by cargo-dist and must never be hand-edited). The
  `github-build-setup` (Node/pnpm bundled-ui step) is preserved by the regen.

### 4. Doc comment touch-ups

`embedder.rs:153` and `lib.rs:10` mention `ort` by name — update to reference the
tract backend. No code/API change.

## Verification

- **No vector-parity gate.** Nothing has been indexed, so tract output need not
  match prior `ort` output. `model_version` stays `"1"`.
- **Correctness:** the existing gated integration test
  `crates/ft-embed/tests/onnx_bge.rs` (real `bge-small-en-v1.5` bundled via git
  LFS, run with `--features onnx -- --ignored` when `FIRETRAIL_BGE_MODEL_DIR`
  is set) is the primary check. It must assert:
  1. the model loads and embeds without error,
  2. the output is 384-dim and unit-norm (‖v‖ ≈ 1),
  3. a semantic-ordering sanity check — a related text pair scores higher cosine
     similarity than an unrelated pair (confirms the graph runs meaningfully, not
     just that it returns numbers).
- **Build/lint:** `cargo build`, `cargo test`, and `cargo clippy` green on the
  default feature set across the workspace. `unsafe_code = "forbid"` continues to
  hold (our code stays safe; tract's internal unsafe is its own crate's concern).
- **Platform proof:** the release build matrix produces an
  `x86_64-apple-darwin` artifact.

## Risks

- **Symbolic input dimension (primary).** tract compiles the plan once, so the
  variable-length sequence axis must be declared symbolic (batch = 1, seq = a
  symbol) rather than fixed. This is the one fiddly piece of the port; the gated
  integration test de-risks it directly. Implement via TDD against that test.
- **Op coverage.** `bge-small-en-v1.5` is a standard BERT encoder (Gather,
  LayerNorm, MatMul, Add, Gelu/Erf, Softmax, Tanh). tract supports these; an
  unsupported-op failure would surface immediately at `into_optimized()` and be
  caught by the integration test.
- **Performance.** tract is typically somewhat slower than native ONNX Runtime.
  Acceptable for a local, single-threaded indexing daemon (ADR-0007); not a
  release gate, but worth a rough note in the test output.

## Out of scope

- No new public API; no model-format or download-path change.
- No rename of the `onnx` feature/type/module.
- No CI workflow *logic* changes beyond the `dist generate` regen.
- No re-embedding / migration tooling (nothing to migrate).
