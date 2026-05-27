//! Deterministic mock embedder used by search/ranking tests.
//!
//! M1: returns an empty vector. M3 will replace this with a deterministic
//! hash-based embedding so search ranking tests can run without loading an
//! ONNX model.

/// Deterministic stand-in for the real M3 embedder.
#[derive(Debug, Clone, Copy)]
pub struct MockEmbedder {
    seed: u64,
}

impl MockEmbedder {
    /// Construct a mock embedder with the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Embed `text` deterministically. Returns `vec![]` at M1.
    #[must_use]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn embed(&self, _text: &str) -> Vec<f32> {
        // Reserved for M3 implementation; intentionally ignores input.
        let _ = self.seed;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_returns_empty_at_m1() {
        let e = MockEmbedder::new(42);
        assert!(e.embed("anything").is_empty());
    }
}
