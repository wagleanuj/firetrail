//! Pluggable embedder configuration and factory (ADR-0007 §"Pluggable
//! embedder").
//!
//! Reads the `embeddings:` section of `.firetrail/config.yml` and builds
//! a boxed [`Embedder`]. The schema:
//!
//! ```yaml
//! embeddings:
//!   provider: local        # local | mock | lexical
//!   model:    bge-small-en-v1.5
//!   fallback: mock         # mock | lexical | none
//! ```
//!
//! - `local`  — loads `OnnxEmbedder` from the machine-local model dir (see
//!   [`crate::download::default_model_dir`]). Requires the `onnx` feature
//!   AND a downloaded model on disk (see [`crate::download`]).
//! - `mock`   — [`MockEmbedder`] (seed 0). Default; deterministic; safe for
//!   tests and offline-first scaffolding.
//! - `lexical`— returns `Ok(None)` from [`build_embedder`] so the caller
//!   degrades to a lexical-only search path (BM25). The
//!   [`EmbeddingsConfig::is_lexical_only`] helper reports this state.
//!
//! When `provider: local` fails (missing model, ONNX feature off), the
//! factory honours `fallback`:
//!
//! - `fallback: mock`    — silently constructs a `MockEmbedder` and records
//!   the substitution as a warning.
//! - `fallback: lexical` — returns `Ok(None)` plus a warning.
//! - `fallback: none`    — propagates the original error.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::embedder::{Embedder, MockEmbedder, OnnxEmbedder};
use crate::error::EmbedError;

/// Provider selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Local ONNX inference (requires the `onnx` feature and a downloaded
    /// model directory).
    Local,
    /// `MockEmbedder` — deterministic, dependency-free. Default.
    #[default]
    Mock,
    /// Caller should skip vector search entirely.
    Lexical,
}

/// Fallback selection when `provider: local` cannot be satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Fallback {
    /// Substitute `MockEmbedder` and emit a warning. Default.
    #[default]
    Mock,
    /// Degrade to lexical-only search and emit a warning.
    Lexical,
    /// Fail loudly. Operators who insist on local inference set this.
    None,
}

/// Resolved embedder configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingsConfig {
    /// Active provider.
    pub provider: Provider,
    /// Model id (used as cache partition key when `provider == Local`).
    /// Defaults to `bge-small-en-v1.5`.
    pub model: String,
    /// Stable model version (used as cache partition key).
    pub model_version: String,
    /// Directory holding the ONNX artifacts. When `None`, defaults to
    /// `<cache_home>/firetrail/models/<model>/`.
    pub model_dir: Option<PathBuf>,
    /// Embedding dimensionality. Defaults to 384 (`bge-small-en-v1.5`).
    pub dim: usize,
    /// What to do if `provider: local` cannot construct a real embedder.
    pub fallback: Fallback,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Mock,
            model: "bge-small-en-v1.5".to_string(),
            model_version: "1".to_string(),
            model_dir: None,
            dim: 384,
            fallback: Fallback::Mock,
        }
    }
}

impl EmbeddingsConfig {
    /// True iff this configuration cannot produce vector embeddings —
    /// callers should run lexical-only search.
    #[must_use]
    pub fn is_lexical_only(&self) -> bool {
        matches!(self.provider, Provider::Lexical)
    }

    /// Read `.firetrail/config.yml` under `workspace_root` and resolve the
    /// `embeddings:` section. Missing file or missing section returns the
    /// default config (`provider: mock`).
    pub fn from_workspace(workspace_root: &Path) -> Result<Self, EmbedError> {
        let path = workspace_root.join(".firetrail").join("config.yml");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        let parsed: WorkspaceConfigFile = serde_yaml::from_str(&raw).map_err(|e| {
            EmbedError::Protocol(format!(
                "parse {}: {e}",
                path.display()
            ))
        })?;
        Ok(parsed.embeddings.unwrap_or_default().resolve())
    }
}

/// Result of [`build_embedder`]: either a real boxed embedder or `None`
/// to signal lexical-only search, plus any warnings recorded during
/// resolution / fallback.
pub struct BuiltEmbedder {
    /// The resolved embedder. `None` means lexical-only (the caller must
    /// not try to vector-search).
    pub embedder: Option<Box<dyn Embedder>>,
    /// Warnings (e.g. "local provider failed, falling back to mock").
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for BuiltEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltEmbedder")
            .field(
                "embedder",
                &self
                    .embedder
                    .as_ref()
                    .map(|e| format!("{}@{}", e.model_id(), e.model_version())),
            )
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// Build a boxed [`Embedder`] from a resolved [`EmbeddingsConfig`].
pub fn build_embedder(cfg: &EmbeddingsConfig) -> Result<BuiltEmbedder, EmbedError> {
    let mut warnings = Vec::new();
    match cfg.provider {
        Provider::Mock => Ok(BuiltEmbedder {
            embedder: Some(Box::new(MockEmbedder::new(0, cfg.dim))),
            warnings,
        }),
        Provider::Lexical => Ok(BuiltEmbedder {
            embedder: None,
            warnings,
        }),
        Provider::Local => {
            let model_dir = match &cfg.model_dir {
                Some(p) => p.clone(),
                None => crate::download::default_model_dir(&cfg.model)?,
            };
            match OnnxEmbedder::load_dir(
                &model_dir,
                cfg.model.clone(),
                cfg.model_version.clone(),
                cfg.dim,
            ) {
                Ok(emb) => Ok(BuiltEmbedder {
                    embedder: Some(Box::new(emb)),
                    warnings,
                }),
                Err(e) => match cfg.fallback {
                    Fallback::Mock => {
                        warnings.push(format!(
                            "local embedder unavailable ({e}); falling back to mock"
                        ));
                        Ok(BuiltEmbedder {
                            embedder: Some(Box::new(MockEmbedder::new(0, cfg.dim))),
                            warnings,
                        })
                    }
                    Fallback::Lexical => {
                        warnings.push(format!(
                            "local embedder unavailable ({e}); falling back to lexical-only"
                        ));
                        Ok(BuiltEmbedder {
                            embedder: None,
                            warnings,
                        })
                    }
                    Fallback::None => Err(e),
                },
            }
        }
    }
}

// ── On-disk YAML shape ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WorkspaceConfigFile {
    embeddings: Option<EmbeddingsSection>,
}

#[derive(Debug, Deserialize, Default)]
struct EmbeddingsSection {
    provider: Option<Provider>,
    model: Option<String>,
    model_version: Option<String>,
    model_dir: Option<PathBuf>,
    dim: Option<usize>,
    fallback: Option<Fallback>,
}

impl EmbeddingsSection {
    fn resolve(self) -> EmbeddingsConfig {
        let d = EmbeddingsConfig::default();
        EmbeddingsConfig {
            provider: self.provider.unwrap_or(d.provider),
            model: self.model.unwrap_or(d.model),
            model_version: self.model_version.unwrap_or(d.model_version),
            model_dir: self.model_dir,
            dim: self.dim.unwrap_or(d.dim),
            fallback: self.fallback.unwrap_or(d.fallback),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_config(root: &Path, yaml: &str) {
        let dir = root.join(".firetrail");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.yml"), yaml).unwrap();
    }

    #[test]
    fn missing_config_yields_default_mock() {
        let dir = tempdir().unwrap();
        let cfg = EmbeddingsConfig::from_workspace(dir.path()).unwrap();
        assert_eq!(cfg.provider, Provider::Mock);
        assert_eq!(cfg.dim, 384);
        assert_eq!(cfg.model, "bge-small-en-v1.5");
    }

    #[test]
    fn missing_embeddings_section_yields_default() {
        let dir = tempdir().unwrap();
        write_config(dir.path(), "storage:\n  mode: embedded\n");
        let cfg = EmbeddingsConfig::from_workspace(dir.path()).unwrap();
        assert_eq!(cfg.provider, Provider::Mock);
    }

    #[test]
    fn local_provider_parses() {
        let dir = tempdir().unwrap();
        write_config(
            dir.path(),
            "embeddings:\n  provider: local\n  model: bge-small-en-v1.5\n  fallback: lexical\n",
        );
        let cfg = EmbeddingsConfig::from_workspace(dir.path()).unwrap();
        assert_eq!(cfg.provider, Provider::Local);
        assert_eq!(cfg.fallback, Fallback::Lexical);
    }

    #[test]
    fn lexical_provider_short_circuits() {
        let dir = tempdir().unwrap();
        write_config(dir.path(), "embeddings:\n  provider: lexical\n");
        let cfg = EmbeddingsConfig::from_workspace(dir.path()).unwrap();
        let built = build_embedder(&cfg).unwrap();
        assert!(built.embedder.is_none());
        assert!(built.warnings.is_empty());
        assert!(cfg.is_lexical_only());
    }

    #[test]
    fn mock_provider_returns_mock_embedder() {
        let cfg = EmbeddingsConfig::default();
        let built = build_embedder(&cfg).unwrap();
        let emb = built.embedder.expect("mock embedder");
        assert_eq!(emb.dim(), 384);
        assert!(emb.model_id().starts_with("mock-"));
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn local_provider_without_onnx_feature_falls_back_to_mock() {
        let dir = tempdir().unwrap();
        write_config(
            dir.path(),
            "embeddings:\n  provider: local\n  fallback: mock\n",
        );
        let cfg = EmbeddingsConfig::from_workspace(dir.path()).unwrap();
        let built = build_embedder(&cfg).unwrap();
        let emb = built.embedder.expect("fallback mock embedder");
        assert!(emb.model_id().starts_with("mock-"));
        assert_eq!(built.warnings.len(), 1, "expected one fallback warning");
        assert!(built.warnings[0].contains("falling back to mock"));
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn local_provider_with_fallback_none_propagates() {
        let dir = tempdir().unwrap();
        write_config(
            dir.path(),
            "embeddings:\n  provider: local\n  fallback: none\n",
        );
        let cfg = EmbeddingsConfig::from_workspace(dir.path()).unwrap();
        let r = build_embedder(&cfg);
        assert!(matches!(r, Err(EmbedError::ModelUnavailable(_))));
    }
}
