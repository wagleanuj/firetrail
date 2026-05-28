//! End-to-end ONNX inference test for `bge-small-en-v1.5`.
//!
//! This test is **ignored by default** (and only compiles with the `onnx`
//! feature). It runs when both of the following are true:
//!
//! - `cargo test --features onnx -- --ignored` is invoked, AND
//! - `$FIRETRAIL_BGE_MODEL_DIR` points at a directory containing both
//!   `model.onnx` and `tokenizer.json` (download via
//!   `firetrail init --download-model` once that flag ships, or fetch
//!   manually with `huggingface-cli download BAAI/bge-small-en-v1.5`).
//!
//! Assertions: the model produces a 384-dim L2-normalised vector;
//! deterministically (two calls yield equal vectors); and semantically
//! related inputs cluster (cosine("dog", "puppy") > cosine("dog",
//! "spaceship")). The thresholds are loose — they verify the pipeline
//! works, not the model's absolute quality.

#![cfg(feature = "onnx")]

use std::path::PathBuf;

use ft_embed::{Embedder, OnnxEmbedder, cosine};

fn model_dir() -> Option<PathBuf> {
    std::env::var_os("FIRETRAIL_BGE_MODEL_DIR").map(PathBuf::from)
}

#[test]
#[ignore = "requires FIRETRAIL_BGE_MODEL_DIR + ~134 MiB model on disk"]
fn onnx_bge_small_round_trips() {
    let Some(dir) = model_dir() else {
        panic!("FIRETRAIL_BGE_MODEL_DIR not set");
    };
    let emb = OnnxEmbedder::load_bge_small(&dir).expect("load model");

    // 1. Dimensionality + normalisation.
    let v = emb.embed("hello world").expect("embed");
    assert_eq!(v.len(), 384, "expected 384-dim vector");
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 1e-3,
        "expected L2-normalised output, got norm={norm}"
    );

    // 2. Determinism.
    let v2 = emb.embed("hello world").expect("embed");
    assert_eq!(v, v2, "two calls must produce the same vector");

    // 3. Sanity: related inputs are more similar than unrelated ones.
    let dog = emb.embed("dog").unwrap();
    let puppy = emb.embed("puppy").unwrap();
    let spaceship = emb.embed("spaceship").unwrap();
    let near = cosine(&dog, &puppy);
    let far = cosine(&dog, &spaceship);
    assert!(
        near > far,
        "expected cos(dog,puppy)={near} > cos(dog,spaceship)={far}"
    );
}
