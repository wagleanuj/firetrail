//! [`EmbedService`] — glues an [`Embedder`] to an [`EmbeddingCache`].
//!
//! All public methods are synchronous: the daemon (see [`crate::daemon`]) is
//! the async layer.

use ft_core::{Record, RecordBody};

use crate::cache::EmbeddingCache;
use crate::embedder::Embedder;
use crate::error::EmbedError;

/// Combine an [`Embedder`] with an [`EmbeddingCache`].
#[derive(Debug)]
pub struct EmbedService<E: Embedder> {
    embedder: E,
    cache: EmbeddingCache,
}

impl<E: Embedder> EmbedService<E> {
    /// Build a service from the given embedder + cache.
    pub fn new(embedder: E, cache: EmbeddingCache) -> Self {
        Self { embedder, cache }
    }

    /// Borrow the underlying embedder.
    pub fn embedder(&self) -> &E {
        &self.embedder
    }

    /// Borrow the underlying cache.
    pub fn cache(&self) -> &EmbeddingCache {
        &self.cache
    }

    /// Embed a raw text blob, going through the cache.
    ///
    /// Computes `content_hash(text)`, looks it up under
    /// `self.embedder.model_id()`, and on miss runs the embedder and caches
    /// the result.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let hash = content_hash(text);
        self.embed_text_with_hash(text, &hash)
    }

    /// Same as [`Self::embed_text`] but with a precomputed content hash —
    /// useful when the caller already computed one.
    pub fn embed_text_with_hash(
        &self,
        text: &str,
        content_hash: &str,
    ) -> Result<Vec<f32>, EmbedError> {
        let model_id = self.embedder.model_id();
        if let Some(v) = self.cache.lookup(model_id, content_hash)? {
            return Ok(v);
        }
        let v = self.embedder.embed(text)?;
        if v.len() != self.embedder.dim() {
            return Err(EmbedError::DimensionMismatch {
                expected: self.embedder.dim(),
                actual: v.len(),
            });
        }
        self.cache.insert(model_id, content_hash, &v)?;
        Ok(v)
    }

    /// Embed a [`Record`]: title + body free-text, content-hashed.
    pub fn embed_record(&self, record: &Record) -> Result<Vec<f32>, EmbedError> {
        let text = record_text(record);
        self.embed_text(&text)
    }
}

/// BLAKE3 content hash of `text`, hex-encoded.
///
/// Stable across machines and architectures. Used as the cache partition key
/// inside [`EmbedService`].
#[must_use]
pub fn content_hash(text: &str) -> String {
    hex::encode(blake3::hash(text.as_bytes()).as_bytes())
}

/// Extract the embeddable text from a [`Record`]: title + the body's prose
/// fields, separated by `"\n\n"`.
///
/// The exact composition is part of the cache contract — changing it
/// invalidates every previously-cached embedding for the same model. Bumping
/// the model id or migrating the cache (ADR-0007) is the supported path.
#[must_use]
pub fn record_text(record: &Record) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(4);
    let title = record.envelope.title.as_str();
    if !title.is_empty() {
        parts.push(title);
    }
    match &record.body {
        RecordBody::Epic(e) => parts.push(&e.description),
        RecordBody::Task(t) => parts.push(&t.description),
        RecordBody::Subtask(s) => parts.push(&s.description),
        RecordBody::Bug(b) => parts.push(&b.description),
        RecordBody::Incident(i) => {
            parts.push(&i.summary);
            if let Some(rc) = &i.root_cause {
                parts.push(rc);
            }
        }
        RecordBody::Finding(f) => {
            parts.push(&f.summary);
            parts.push(&f.details);
        }
        RecordBody::Runbook(r) => {
            parts.push(&r.title);
            parts.push(&r.summary);
        }
        RecordBody::Decision(d) => {
            parts.push(&d.title);
            parts.push(&d.context);
            parts.push(&d.decision);
            parts.push(&d.consequences);
        }
        RecordBody::Gotcha(g) => {
            parts.push(&g.summary);
            parts.push(&g.details);
        }
        RecordBody::Memory(m) => {
            parts.push(&m.title);
            parts.push(&m.body);
        }
    }
    parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;

    use super::*;
    use crate::embedder::MockEmbedder;

    /// Embedder wrapper that counts how often `embed()` is called — lets us
    /// prove the cache is hit on the second call.
    struct CountingEmbedder {
        inner: MockEmbedder,
        calls: AtomicUsize,
    }

    impl CountingEmbedder {
        fn new() -> Self {
            Self {
                inner: MockEmbedder::new(0, 16),
                calls: AtomicUsize::new(0),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Embedder for CountingEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.embed(text)
        }
        fn dim(&self) -> usize {
            self.inner.dim()
        }
        fn model_id(&self) -> &str {
            self.inner.model_id()
        }
    }

    fn open_cache() -> (tempfile::TempDir, EmbeddingCache) {
        let dir = tempdir().unwrap();
        let cache = EmbeddingCache::open(dir.path().join("e.db")).unwrap();
        (dir, cache)
    }

    #[test]
    fn content_hash_is_deterministic_and_stable() {
        let a = content_hash("firetrail");
        let b = content_hash("firetrail");
        assert_eq!(a, b);
        // Stability: BLAKE3 of "firetrail" — keeping the assertion loose
        // (length + hex) so we don't accidentally pin to a non-stable choice.
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, content_hash("not-firetrail"));
    }

    #[test]
    fn service_caches_so_second_call_does_not_recompute() {
        let (_dir, cache) = open_cache();
        let svc = EmbedService::new(CountingEmbedder::new(), cache);
        let v1 = svc.embed_text("hello").unwrap();
        let v2 = svc.embed_text("hello").unwrap();
        assert_eq!(v1, v2);
        assert_eq!(svc.embedder().calls(), 1, "second call must hit cache");
    }

    #[test]
    fn service_partitions_cache_by_model_id() {
        let (_dir, cache_dir) = tempdir()
            .map(|d| {
                let p = d.path().join("e.db");
                (d, p)
            })
            .unwrap();
        let cache_a = EmbeddingCache::open(&cache_dir).unwrap();
        let svc_a = EmbedService::new(MockEmbedder::new(1, 8), cache_a);
        let svc_b = EmbedService::new(
            MockEmbedder::new(2, 8),
            EmbeddingCache::open(&cache_dir).unwrap(),
        );
        let a = svc_a.embed_text("x").unwrap();
        let b = svc_b.embed_text("x").unwrap();
        assert_ne!(a, b);
    }
}
