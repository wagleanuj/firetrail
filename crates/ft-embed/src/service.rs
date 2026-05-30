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

/// Freshness of a file-backed [`ft_core::Doc`] relative to the file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFreshness {
    /// The file's current content hash matches the record's `content_hash`.
    Fresh,
    /// The file exists but changed since the record was last indexed.
    Stale,
    /// The linked file is missing or unreadable (a broken link).
    Missing,
}

/// Compare a `Doc`'s stored `content_hash` against the linked file's current
/// hash. `doc.path` is resolved against the workspace `root`.
///
/// This is the drift signal the lazy-refresh path uses: [`DocFreshness::Stale`]
/// means re-index, [`DocFreshness::Missing`] means render a broken link. The
/// hash is computed over the raw file contents, so writers must hash the same
/// (see `firetrail doc add/index`).
#[must_use]
pub fn doc_freshness(root: &std::path::Path, doc: &ft_core::Doc) -> DocFreshness {
    match std::fs::read_to_string(root.join(&doc.path)) {
        Ok(content) if content_hash(&content) == doc.content_hash => DocFreshness::Fresh,
        Ok(_) => DocFreshness::Stale,
        Err(_) => DocFreshness::Missing,
    }
}

/// Re-derive a [`ft_core::Doc`] record's `content_hash` + `summary` from file
/// `content`.
///
/// Returns `true` when either field changed (the caller must persist the
/// record). A non-doc record returns `false`. This is the single source of
/// truth for doc-content derivation shared by `firetrail doc add/index` and the
/// ft-ui edit-through path, so the two can never compute a hash differently.
#[must_use]
pub fn apply_doc_content(record: &mut Record, content: &str) -> bool {
    let RecordBody::Doc(doc) = &mut record.body else {
        return false;
    };
    let new_hash = content_hash(content);
    let (_title, summary) = parse_doc_meta(content);
    if new_hash == doc.content_hash && summary == doc.summary {
        return false;
    }
    doc.content_hash = new_hash;
    doc.summary = summary;
    true
}

/// Extract `(title, summary)` from doc markdown: skips a leading YAML
/// frontmatter block, takes the first `# H1` as the title and the first prose
/// paragraph as the summary (capped at 280 chars so the record stays a thin
/// pointer at the file).
#[must_use]
pub fn parse_doc_meta(text: &str) -> (Option<String>, String) {
    let body = strip_frontmatter(text);
    let mut title = None;
    let mut summary = String::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h1) = line.strip_prefix("# ") {
            if title.is_none() {
                title = Some(h1.trim().to_string());
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        summary = line.to_string();
        break;
    }
    if summary.len() > 280 {
        summary.truncate(277);
        summary.push_str("...");
    }
    (title, summary)
}

/// Drop a leading `---\n … \n---` YAML frontmatter block if present.
fn strip_frontmatter(text: &str) -> &str {
    let t = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"));
    if let Some(rest) = t {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.trim_start_matches(['\r', '\n']);
        }
    }
    text
}

/// Machine-readable fields a doc may declare in its YAML frontmatter
/// (firetrail docs design spec §5). All are optional: a doc with no
/// frontmatter — or none of these keys — yields the [`Default`].
///
/// `links` carries the work-item ids this doc documents; the doc-add path
/// turns each into a `work_item --DocumentedIn--> doc` edge. Unknown keys are
/// ignored and an unrecognized `status` value leaves [`status`](Self::status)
/// `None` (the record stays `Draft`) rather than erroring.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocFrontmatter {
    /// `doc_type:` — overrides the `--type` flag when present.
    pub doc_type: Option<String>,
    /// `status:` — mapped to a [`ft_core::TrustState`]; `None` if absent or
    /// unrecognized.
    pub status: Option<ft_core::TrustState>,
    /// `scope:` — owning scope for the record envelope.
    pub scope: Option<String>,
    /// `links:` — work-item ids this doc documents (block or inline list).
    pub links: Vec<String>,
}

/// Parse the [`DocFrontmatter`] from a doc's markdown.
///
/// Hand-parses a deliberately small, flat subset of YAML — `key: scalar`
/// pairs plus a `links:` list in either block (`- item`) or inline
/// (`[a, b]`) form. Trailing `" #"` comments and surrounding quotes are
/// stripped. Anything outside this subset is ignored, never an error: the
/// goal is a forgiving convention layer, not a YAML engine.
#[must_use]
pub fn parse_frontmatter(text: &str) -> DocFrontmatter {
    let mut fm = DocFrontmatter::default();
    let Some(block) = frontmatter_block(text) else {
        return fm;
    };

    let mut lines = block.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = strip_comment(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "doc_type" if !value.is_empty() => fm.doc_type = Some(value.to_string()),
            "scope" if !value.is_empty() => fm.scope = Some(value.to_string()),
            "status" => fm.status = parse_trust_state(value),
            "links" => {
                if value.is_empty() {
                    // Block list: consume the following `  - item` lines.
                    while let Some(peek) = lines.peek() {
                        let item_line = strip_comment(peek);
                        let item = item_line.trim();
                        if let Some(rest) = item.strip_prefix("- ") {
                            let id = unquote(rest.trim());
                            if !id.is_empty() {
                                fm.links.push(id.to_string());
                            }
                            lines.next();
                        } else if item.is_empty() {
                            lines.next();
                        } else {
                            break; // next key — leave it for the outer loop.
                        }
                    }
                } else {
                    fm.links.extend(parse_inline_list(value));
                }
            }
            _ => {}
        }
    }
    fm
}

/// The inner text of a leading `---\n … \n---` frontmatter block, if present.
fn frontmatter_block(text: &str) -> Option<&str> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Drop a trailing `" #…"` YAML comment from a line.
fn strip_comment(line: &str) -> &str {
    match line.find(" #") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Strip a single pair of matching surrounding quotes.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Map a `status:` value to a [`ft_core::TrustState`] (the `snake_case` wire
/// names). Unrecognized values yield `None`.
fn parse_trust_state(s: &str) -> Option<ft_core::TrustState> {
    use ft_core::TrustState;
    Some(match s.trim() {
        "draft" => TrustState::Draft,
        "reviewed" => TrustState::Reviewed,
        "verified" => TrustState::Verified,
        "stale" => TrustState::Stale,
        "deprecated" => TrustState::Deprecated,
        "archived" => TrustState::Archived,
        "superseded" => TrustState::Superseded,
        "rejected" => TrustState::Rejected,
        "redacted" => TrustState::Redacted,
        _ => return None,
    })
}

/// Split an inline `[a, b]` (or bare comma-scalar) list into trimmed,
/// unquoted, non-empty items.
fn parse_inline_list(value: &str) -> Vec<String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .map(|s| unquote(s.trim()))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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

    #[test]
    fn doc_freshness_detects_fresh_stale_and_missing() {
        use ft_core::Doc;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("d.md"), "original").unwrap();
        let mut doc = Doc {
            path: "d.md".into(),
            content_hash: content_hash("original"),
            title: "d".into(),
            summary: String::new(),
            doc_type: "design".into(),
            trust: ft_core::TrustState::Draft,
        };
        assert_eq!(doc_freshness(dir.path(), &doc), DocFreshness::Fresh);

        std::fs::write(dir.path().join("d.md"), "edited out of band").unwrap();
        assert_eq!(doc_freshness(dir.path(), &doc), DocFreshness::Stale);

        doc.path = "gone.md".into();
        assert_eq!(doc_freshness(dir.path(), &doc), DocFreshness::Missing);
    }

    #[test]
    fn parse_doc_meta_skips_frontmatter_and_heading() {
        let md = "---\ndoc_type: design\nlinks:\n  - x\n---\n# The Title\n\nThe first prose paragraph.\nmore.\n";
        let (title, summary) = super::parse_doc_meta(md);
        assert_eq!(title.as_deref(), Some("The Title"));
        assert_eq!(summary, "The first prose paragraph.");
    }

    #[test]
    fn parse_doc_meta_no_frontmatter_no_heading() {
        let (title, summary) = super::parse_doc_meta("just a body line\nsecond");
        assert_eq!(title, None);
        assert_eq!(summary, "just a body line");
    }

    #[test]
    fn parse_doc_meta_summary_is_capped() {
        let long = format!("# T\n\n{}", "x".repeat(400));
        let (_t, summary) = super::parse_doc_meta(&long);
        assert!(summary.len() <= 280);
        assert!(summary.ends_with("..."));
    }

    #[test]
    fn parse_frontmatter_reads_scalars_block_list_and_maps_status() {
        let md = "---\n\
            doc_type: adr\n\
            status: reviewed\n\
            scope: ft-ui\n\
            links:\n\
            \x20 - firetrail-n3gh\n\
            \x20 - firetrail-2mwp\n\
            ---\n\
            # Title\n\nBody.\n";
        let fm = super::parse_frontmatter(md);
        assert_eq!(fm.doc_type.as_deref(), Some("adr"));
        assert_eq!(fm.status, Some(ft_core::TrustState::Reviewed));
        assert_eq!(fm.scope.as_deref(), Some("ft-ui"));
        assert_eq!(fm.links, vec!["firetrail-n3gh", "firetrail-2mwp"]);
    }

    #[test]
    fn parse_frontmatter_reads_inline_list_and_strips_comments_and_quotes() {
        let md = "---\n\
            doc_type: \"design\"  # the kind\n\
            links: [firetrail-a, firetrail-b]\n\
            ---\nbody\n";
        let fm = super::parse_frontmatter(md);
        assert_eq!(fm.doc_type.as_deref(), Some("design"));
        assert_eq!(fm.links, vec!["firetrail-a", "firetrail-b"]);
        assert_eq!(fm.status, None);
        assert_eq!(fm.scope, None);
    }

    #[test]
    fn parse_frontmatter_unknown_status_leaves_none() {
        let fm = super::parse_frontmatter("---\nstatus: bananas\n---\nbody\n");
        assert_eq!(fm.status, None, "unknown status value is ignored, not an error");
    }

    #[test]
    fn parse_frontmatter_absent_block_yields_empty() {
        let fm = super::parse_frontmatter("# Just a heading\n\nNo frontmatter here.\n");
        assert_eq!(fm.doc_type, None);
        assert_eq!(fm.status, None);
        assert_eq!(fm.scope, None);
        assert!(fm.links.is_empty());
    }

    #[test]
    fn parse_frontmatter_maps_all_known_statuses() {
        let cases = [
            ("draft", ft_core::TrustState::Draft),
            ("reviewed", ft_core::TrustState::Reviewed),
            ("verified", ft_core::TrustState::Verified),
            ("stale", ft_core::TrustState::Stale),
            ("deprecated", ft_core::TrustState::Deprecated),
            ("archived", ft_core::TrustState::Archived),
            ("superseded", ft_core::TrustState::Superseded),
            ("rejected", ft_core::TrustState::Rejected),
            ("redacted", ft_core::TrustState::Redacted),
        ];
        for (raw, want) in cases {
            let md = format!("---\nstatus: {raw}\n---\n");
            assert_eq!(super::parse_frontmatter(&md).status, Some(want), "status {raw}");
        }
    }

    #[test]
    fn apply_doc_content_updates_hash_and_summary_only_on_change() {
        use ft_core::{Doc, RecordBuilder, RecordKind};
        let mut record = RecordBuilder::new(
            RecordKind::Doc,
            "d",
            ft_core::Identity::new("a@b.test").unwrap(),
        )
        .doc(Doc {
            path: "d.md".into(),
            content_hash: content_hash("# T\n\nold body\n"),
            title: "d".into(),
            summary: "old body".into(),
            doc_type: "design".into(),
            trust: ft_core::TrustState::Draft,
        })
        .build()
        .unwrap();

        // Identical content → no change.
        assert!(!super::apply_doc_content(&mut record, "# T\n\nold body\n"));

        // New content → hash + summary update.
        assert!(super::apply_doc_content(&mut record, "# T\n\nnew body\n"));
        if let ft_core::RecordBody::Doc(doc) = &record.body {
            assert_eq!(doc.summary, "new body");
            assert_eq!(doc.content_hash, content_hash("# T\n\nnew body\n"));
        } else {
            panic!("expected Doc body");
        }
    }
}
