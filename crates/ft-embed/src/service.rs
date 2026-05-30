//! [`EmbedService`] — glues an [`Embedder`] to an [`EmbeddingCache`].
//!
//! All public methods are synchronous: the daemon (see [`crate::daemon`]) is
//! the async layer.

use ft_core::{Record, RecordBody};

use crate::cache::EmbeddingCache;
use crate::embedder::Embedder;
use crate::error::EmbedError;

/// Outcome of [`EmbedService::detect_drift`]: how many sampled rows were
/// re-embedded, and which ones diverged beyond the cosine tolerance.
#[derive(Debug, Clone, PartialEq)]
pub struct DriftReport {
    /// Rows sampled from the cache.
    pub sampled: usize,
    /// Rows actually re-embedded (sample minus skips).
    pub compared: usize,
    /// Rows skipped because their text was unavailable to the caller, or
    /// because their `model_id`/`model_version` does not match the live
    /// embedder.
    pub skipped: usize,
    /// Rows whose cosine similarity vs the re-embedded vector fell below
    /// the configured tolerance.
    pub drifted: Vec<DriftIssue>,
    /// Cosine-similarity floor used during the run (e.g. `0.999`).
    pub tolerance: f32,
}

/// One row that drifted: cached vs newly-embedded cosine similarity below
/// tolerance.
#[derive(Debug, Clone, PartialEq)]
pub struct DriftIssue {
    /// Model id of the drifted row.
    pub model_id: String,
    /// Model version of the drifted row.
    pub model_version: String,
    /// Content hash of the drifted row.
    pub content_hash: String,
    /// Cosine similarity between cached and re-embedded vectors.
    pub cosine: f32,
}

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
    /// `(self.embedder.model_id(), self.embedder.model_version())`, and on
    /// miss runs the embedder and caches the result.
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
        let model_version = self.embedder.model_version();
        if let Some(v) = self.cache.lookup(model_id, model_version, content_hash)? {
            return Ok(v);
        }
        let v = self.embedder.embed(text)?;
        if v.len() != self.embedder.dim() {
            return Err(EmbedError::DimensionMismatch {
                expected: self.embedder.dim(),
                actual: v.len(),
            });
        }
        self.cache
            .insert(model_id, model_version, content_hash, &v)?;
        Ok(v)
    }

    /// Embed a [`Record`]: title + body free-text, content-hashed.
    pub fn embed_record(&self, record: &Record) -> Result<Vec<f32>, EmbedError> {
        let text = record_text(record);
        self.embed_text(&text)
    }

    /// Sample up to `n` cached rows, re-embed them via `text_for_hash`
    /// (caller-supplied: maps a content hash back to the original source
    /// text, typically by reading the index/storage), and compare cosine
    /// similarity to the cached vector. Rows whose hash has no known text,
    /// or whose `(model_id, model_version)` doesn't match the live
    /// embedder, are counted as skipped.
    ///
    /// `tolerance` is the cosine-similarity floor — sampled rows with
    /// `cosine < tolerance` are reported as drift. A reasonable starting
    /// value for `bge-small-en-v1.5` is `0.999` (deterministic ONNX
    /// inference should round-trip far closer than that).
    pub fn detect_drift(
        &self,
        n: usize,
        tolerance: f32,
        text_for_hash: impl Fn(&str) -> Option<String>,
    ) -> Result<DriftReport, EmbedError> {
        let sample = self.cache.sample_for_reembed(n)?;
        let sampled = sample.len();
        let mut compared = 0usize;
        let mut skipped = 0usize;
        let mut drifted = Vec::new();
        for row in sample {
            if row.model_id != self.embedder.model_id()
                || row.model_version != self.embedder.model_version()
            {
                skipped += 1;
                continue;
            }
            let Some(text) = text_for_hash(&row.content_hash) else {
                skipped += 1;
                continue;
            };
            let recomputed = self.embedder.embed(&text)?;
            compared += 1;
            let cos = cosine(&row.embedding, &recomputed);
            if cos < tolerance {
                drifted.push(DriftIssue {
                    model_id: row.model_id,
                    model_version: row.model_version,
                    content_hash: row.content_hash,
                    cosine: cos,
                });
            }
        }
        Ok(DriftReport {
            sampled,
            compared,
            skipped,
            drifted,
            tolerance,
        })
    }
}

/// Cosine similarity between two equal-length vectors. Returns `0.0` for
/// length mismatch or zero-norm inputs (defensive — drift detection treats
/// degenerate vectors as "skip", not "drift").
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
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
        // TODO(firetrail-2mwp.4): embed the linked .md file contents, not just
        // the stored summary. Interim: title (envelope) + summary keeps Docs
        // searchable until file-backed extraction lands.
        RecordBody::Doc(d) => {
            parts.push(&d.title);
            parts.push(&d.summary);
        }
    }
    parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Like [`record_text`], but for file-backed [`Doc`] records it embeds the
/// linked `.md` file's contents (resolved against `root`) rather than the
/// stored summary excerpt.
///
/// A missing or unreadable file degrades to [`record_text`] (title + summary)
/// rather than failing — a broken doc link must not poison indexing.
#[must_use]
pub fn record_text_with_root(root: &std::path::Path, record: &Record) -> String {
    if let RecordBody::Doc(d) = &record.body {
        if let Ok(content) = std::fs::read_to_string(root.join(&d.path)) {
            let title = record.envelope.title.as_str();
            return [title, content.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
        }
    }
    record_text(record)
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
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("e.db");
        let cache_a = EmbeddingCache::open(&cache_path).unwrap();
        let svc_a = EmbedService::new(MockEmbedder::new(1, 8), cache_a);
        let svc_b = EmbedService::new(
            MockEmbedder::new(2, 8),
            EmbeddingCache::open(&cache_path).unwrap(),
        );
        let a = svc_a.embed_text("x").unwrap();
        let b = svc_b.embed_text("x").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn detect_drift_reports_no_drift_for_pristine_cache() {
        let (_dir, cache) = open_cache();
        let svc = EmbedService::new(MockEmbedder::new(0, 16), cache);
        // Populate.
        let texts = ["alpha", "beta", "gamma"];
        for t in texts {
            svc.embed_text(t).unwrap();
        }
        let map: std::collections::HashMap<String, String> = texts
            .iter()
            .map(|t| (content_hash(t), (*t).to_string()))
            .collect();
        let r = svc
            .detect_drift(10, 0.999, |h| map.get(h).cloned())
            .unwrap();
        assert_eq!(r.compared, 3);
        assert_eq!(r.skipped, 0);
        assert!(r.drifted.is_empty(), "{r:?}");
    }

    #[test]
    fn detect_drift_flags_silent_drift() {
        let (_dir, cache) = open_cache();
        let svc = EmbedService::new(MockEmbedder::new(0, 16), cache);
        svc.embed_text("alpha").unwrap();
        // Replace the cached vector with something orthogonal-ish and
        // re-checksum so integrity_check still passes — the only way to
        // catch this is to re-embed and compare.
        let h = content_hash("alpha");
        let model_id = svc.embedder().model_id().to_string();
        let model_version = svc.embedder().model_version().to_string();
        let mut fake = vec![0.0_f32; 16];
        fake[0] = 1.0;
        svc.cache()
            .drift_for_test(&model_id, &model_version, &h, &fake)
            .unwrap();

        let map: std::collections::HashMap<String, String> =
            [(h.clone(), "alpha".to_string())].into_iter().collect();
        let r = svc
            .detect_drift(10, 0.999, |h| map.get(h).cloned())
            .unwrap();
        assert_eq!(r.compared, 1);
        assert_eq!(r.drifted.len(), 1, "{r:?}");
        assert_eq!(r.drifted[0].content_hash, h);
        assert!(r.drifted[0].cosine < 0.999);
    }

    #[test]
    fn detect_drift_skips_rows_for_other_models() {
        let (_dir, cache) = open_cache();
        // Pre-populate the cache via a different embedder identity.
        cache.insert("other-model", "1", "h", &[1.0; 16]).unwrap();
        let svc = EmbedService::new(MockEmbedder::new(0, 16), cache);
        let r = svc
            .detect_drift(10, 0.999, |_| Some("alpha".to_string()))
            .unwrap();
        assert_eq!(r.sampled, 1);
        assert_eq!(r.compared, 0);
        assert_eq!(r.skipped, 1);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn cosine_handles_degenerate_inputs() {
        // These cases produce exact-zero outputs by construction (early
        // return on length mismatch / zero norm), so an exact compare is
        // intentional here.
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    }

    fn doc_record(path: &str, summary: &str) -> Record {
        use ft_core::{Doc, Identity, RecordBuilder, RecordKind, TrustState};
        RecordBuilder::new(
            RecordKind::Doc,
            "Design doc",
            Identity::new("a@b.com").unwrap(),
        )
        .doc(Doc {
            path: path.into(),
            content_hash: String::new(),
            title: "Design doc".into(),
            summary: summary.into(),
            doc_type: "design".into(),
            trust: TrustState::Reviewed,
        })
        .build()
        .unwrap()
    }

    #[test]
    fn record_text_with_root_embeds_file_contents() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("design.md"),
            "# Plan\nThe quokka subsystem uses a ringbuffer.",
        )
        .unwrap();
        let rec = doc_record("design.md", "short excerpt");

        let text = record_text_with_root(dir.path(), &rec);
        // Full file content is embedded, not just the summary.
        assert!(text.contains("quokka subsystem uses a ringbuffer"));
        assert!(!text.contains("short excerpt"));
    }

    #[test]
    fn record_text_with_root_falls_back_when_file_missing() {
        let dir = tempdir().unwrap();
        let rec = doc_record("does-not-exist.md", "short excerpt");
        // No panic; degrades to the summary-based record_text.
        let text = record_text_with_root(dir.path(), &rec);
        assert_eq!(text, record_text(&rec));
        assert!(text.contains("short excerpt"));
    }
}
