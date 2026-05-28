//! `firetrail migrate embeddings` (firetrail-vpn).
//!
//! Re-embed every record in storage under a target model and write a
//! deterministic JSONL artifact. The output file is the resume marker —
//! if interrupted, the next invocation reads already-processed ids from
//! the partial output and only embeds the remainder.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ft_embed::{
    EmbedService, EmbeddingCache, EmbeddingsConfig, MockEmbedder, OnnxEmbedder, Provider,
    record_text,
};
use ft_storage::{EmbeddedStorage, Storage as _, StorageFilter};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::cli::{GlobalOpts, MigrateEmbeddingsArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const CMD: &str = "migrate embeddings";

/// `firetrail migrate embeddings --to <model>`
pub fn embeddings(
    args: &MigrateEmbeddingsArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD, global.workspace.as_deref())?;
    let storage = EmbeddedStorage::open(&ws.root).map_err(|e| CliError::internal(CMD, e))?;

    let cfg = EmbeddingsConfig::from_workspace(&ws.root)
        .map_err(|e| CliError::internal(CMD, format!("load embeddings config: {e}")))?;
    let embedder = build_target_embedder(&cfg, args)?;
    // The artifact records the user-requested model id/version so the
    // resulting JSONL is portable even when the underlying embedder is a
    // mock (the embedder's auto-derived id may not match `--to`).
    let model_id = args.to.clone();
    let model_version = args.version.clone().unwrap_or_else(|| "1".to_string());

    let cache = EmbeddingCache::open_under(&ws.root)
        .map_err(|e| CliError::internal(CMD, format!("open cache: {e}")))?;
    let service = EmbedService::new(embedder, cache);

    // Resume: if the output already contains rows, capture the set of done
    // ids so we skip them on a re-run. `force` blows the file away.
    let already_done = if args.force {
        let _ = std::fs::remove_file(&args.output);
        std::collections::HashSet::new()
    } else {
        load_done_ids(&args.output).map_err(|e| CliError::internal(CMD, e))?
    };

    let ids = storage
        .list(&StorageFilter::default())
        .map_err(|e| CliError::internal(CMD, format!("list records: {e}")))?;
    let total = ids.len();

    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::internal(CMD, e))?;
        }
    }
    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args.output)
        .map_err(|e| CliError::internal(CMD, format!("open output: {e}")))?;

    let started = Instant::now();
    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut digest = Sha256::new();

    for id in &ids {
        if already_done.contains(id.as_str()) {
            skipped += 1;
            continue;
        }
        let record = storage
            .read(id)
            .map_err(|e| CliError::internal(CMD, format!("read {id}: {e}")))?;
        let text = record_text(&record);
        let embedding = service
            .embed_text(&text)
            .map_err(|e| CliError::internal(CMD, format!("embed {id}: {e}")))?;
        if embedding.len() != args.dim {
            return Err(CliError::user(
                CMD,
                format!(
                    "model produced dim {} but --dim {}; refusing to write inconsistent artifact",
                    embedding.len(),
                    args.dim
                ),
            ));
        }
        let row = EmbeddingRow {
            id: id.as_str().to_string(),
            model_id: model_id.clone(),
            model_version: model_version.clone(),
            dim: embedding.len(),
            embedding,
        };
        let line =
            serde_json::to_string(&row).map_err(|e| CliError::internal(CMD, e.to_string()))?;
        digest.update(line.as_bytes());
        digest.update(b"\n");
        writeln!(out, "{line}").map_err(|e| CliError::internal(CMD, e.to_string()))?;
        out.flush().map_err(|e| CliError::internal(CMD, e.to_string()))?;
        written += 1;
    }

    // Recompute the full-file digest so the manifest reflects skipped+written
    // (resumed runs need the same final hash as a one-shot run).
    let artifact_sha256 = full_file_digest(&args.output).map_err(|e| CliError::internal(CMD, e))?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let outcome = MigrateEmbeddingsOutcome {
        command: CMD,
        model_id: model_id.clone(),
        model_version,
        dim: args.dim,
        total,
        written,
        skipped,
        elapsed_ms,
        output_path: args.output.clone(),
        artifact_sha256,
        warnings: Vec::new(),
    };
    Ok(CommandOutcome::Migrate(outcome))
}

fn build_target_embedder(
    cfg: &EmbeddingsConfig,
    args: &MigrateEmbeddingsArgs,
) -> Result<Box<dyn ft_embed::Embedder>, CliError> {
    match cfg.provider {
        Provider::Local => {
            let model_dir = cfg.model_dir.clone().ok_or_else(|| {
                CliError::user(
                    CMD,
                    "provider: local has no model_dir configured; cannot migrate to an ONNX target",
                )
            })?;
            let version = args.version.clone().unwrap_or_else(|| "1".to_string());
            let onnx = OnnxEmbedder::load_dir(&model_dir, args.to.clone(), version, args.dim)
                .map_err(|e| CliError::internal(CMD, format!("load ONNX model: {e}")))?;
            Ok(Box::new(onnx))
        }
        Provider::Mock => Ok(Box::new(MockEmbedder::new(0, args.dim))),
        Provider::Lexical => Err(CliError::user(
            CMD,
            "embeddings.provider=lexical: nothing to migrate. Switch to provider: mock or local first.",
        )),
    }
}

fn load_done_ids(path: &Path) -> Result<std::collections::HashSet<String>, String> {
    let mut set = std::collections::HashSet::new();
    if !path.exists() {
        return Ok(set);
    }
    let f = std::fs::File::open(path).map_err(|e| format!("open partial output: {e}"))?;
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| format!("read partial output: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let row: EmbeddingRow =
            serde_json::from_str(&line).map_err(|e| format!("parse partial output: {e}"))?;
        set.insert(row.id);
    }
    Ok(set)
}

fn full_file_digest(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("open output for hash: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read output for hash: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct EmbeddingRow {
    id: String,
    model_id: String,
    model_version: String,
    dim: usize,
    embedding: Vec<f32>,
}

/// Outcome of a `migrate embeddings` run.
#[derive(Debug, Clone, Serialize)]
pub struct MigrateEmbeddingsOutcome {
    #[serde(skip)]
    pub command: &'static str,
    pub model_id: String,
    pub model_version: String,
    pub dim: usize,
    pub total: usize,
    pub written: usize,
    pub skipped: usize,
    pub elapsed_ms: u64,
    pub output_path: PathBuf,
    pub artifact_sha256: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl MigrateEmbeddingsOutcome {
    pub fn markdown(&self) -> String {
        format!(
            "**migrate embeddings** model={} dim={} written={}/{} skipped={} sha256={} output=`{}`\n",
            self.model_id,
            self.dim,
            self.written,
            self.total,
            self.skipped,
            &self.artifact_sha256[..16],
            self.output_path.display()
        )
    }

    pub fn quiet_line(&self) -> String {
        format!(
            "migrate embeddings: {}/{} written ({} skipped)",
            self.written, self.total, self.skipped
        )
    }
}

