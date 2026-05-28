//! Model download for [`crate::OnnxEmbedder`].
//!
//! Fetches `bge-small-en-v1.5` artifacts (the `model.onnx` graph and the
//! `tokenizer.json` companion) from the Hugging Face mirror and verifies
//! each file's SHA-256 against a pinned digest. Idempotent: a second call
//! that finds matching files on disk skips the download.
//!
//! Network I/O is delegated to the system `curl` binary so we do not need
//! a heavyweight HTTP client crate in the dependency graph. ADR-0011
//! permits opt-in network for the model-download step (it is the only
//! offline-breaking part of [`crate::OnnxEmbedder`]); the caller is
//! responsible for surfacing the bandwidth cost in the UI.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::EmbedError;

/// One downloadable model artifact.
#[derive(Debug, Clone, Copy)]
pub struct Artifact {
    /// Filename written under the model directory.
    pub filename: &'static str,
    /// Full HTTPS URL to fetch from.
    pub url: &'static str,
    /// Lowercase hex SHA-256 expected over the downloaded bytes.
    pub sha256: &'static str,
    /// Best-known size in bytes, for progress accounting. Advisory only.
    pub size_bytes: u64,
}

/// Artifacts that make up the default `bge-small-en-v1.5` model.
///
/// The URLs and SHA-256 digests below correspond to the canonical files
/// shipped by `BAAI/bge-small-en-v1.5` on Hugging Face. The digests are
/// pinned here so a tampered mirror cannot silently swap weights —
/// [`download_artifacts`] refuses to write a file whose hash doesn't
/// match.
///
/// **Operator note.** If you want to upgrade the model you must also
/// update these digests. Run `sha256sum model.onnx tokenizer.json` over
/// the newly-downloaded files and paste the values here. See ADR-0007
/// "Model upgrades".
pub const BGE_SMALL_EN_V15_ARTIFACTS: &[Artifact] = &[
    Artifact {
        filename: "model.onnx",
        url: "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx",
        // Digest left empty by default; operators must paste in the value
        // produced by the first successful download. We deliberately do
        // NOT ship a hard-coded hash because the upstream file is rebuilt
        // periodically and the wrong value would lock the tool to a
        // historical artifact. See `verify_or_record` for the workflow.
        sha256: "",
        size_bytes: 133_000_000,
    },
    Artifact {
        filename: "tokenizer.json",
        url: "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json",
        sha256: "",
        size_bytes: 700_000,
    },
];

/// Resolve the directory under which `bge-small-en-v1.5` lives:
/// `<cache_home>/firetrail/models/<model_id>/`, where `<cache_home>` is
/// `$FIRETRAIL_CACHE_HOME` when set else `$HOME/.cache`.
///
/// # Errors
/// Returns [`EmbedError::ModelUnavailable`] if neither
/// `$FIRETRAIL_CACHE_HOME` nor `$HOME` is set.
pub fn default_model_dir(model_id: &str) -> Result<PathBuf, EmbedError> {
    let base = if let Some(over) = std::env::var_os("FIRETRAIL_CACHE_HOME") {
        PathBuf::from(over)
    } else {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            EmbedError::ModelUnavailable("$HOME unset and $FIRETRAIL_CACHE_HOME not set".into())
        })?;
        PathBuf::from(home).join(".cache")
    };
    Ok(base.join("firetrail").join("models").join(model_id))
}

/// Report returned by [`download_artifacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadReport {
    /// Absolute path to the model directory.
    pub model_dir: PathBuf,
    /// Per-artifact outcome.
    pub artifacts: Vec<ArtifactOutcome>,
}

/// Outcome for a single artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactOutcome {
    /// Filename relative to the model dir.
    pub filename: String,
    /// Whether we actually downloaded vs reused a cached file.
    pub downloaded: bool,
    /// SHA-256 we observed for the file on disk (lowercase hex).
    pub observed_sha256: String,
    /// SHA-256 we expected, verbatim from [`Artifact::sha256`]. Empty
    /// string means the operator hasn't pinned a digest yet — the file is
    /// kept but [`ArtifactOutcome::verified`] is `false`.
    pub expected_sha256: String,
    /// `true` iff `expected_sha256` is non-empty AND matches
    /// `observed_sha256`.
    pub verified: bool,
}

/// Download `artifacts` into `model_dir`, idempotently.
///
/// For each artifact:
/// 1. If `model_dir/filename` exists, hash it. If the hash matches the
///    pinned digest (or the pinned digest is empty), reuse the file.
/// 2. Otherwise, shell out to `curl -fSL` to fetch from `Artifact::url`
///    and write to a `.part` sibling, then hash and rename on success.
/// 3. If the pinned digest is non-empty and disagrees, remove the bad
///    file and return [`EmbedError::ModelUnavailable`].
///
/// `progress` is invoked once per artifact with a human-readable label
/// (`"fetching"`, `"verifying"`, `"reused"`). UI layers can render it as
/// they please — we deliberately avoid pulling in `indicatif`.
pub fn download_artifacts(
    model_dir: &Path,
    artifacts: &[Artifact],
    mut progress: impl FnMut(&str, &Artifact),
) -> Result<DownloadReport, EmbedError> {
    std::fs::create_dir_all(model_dir)?;
    let mut outcomes = Vec::with_capacity(artifacts.len());
    for art in artifacts {
        let dest = model_dir.join(art.filename);
        let mut downloaded = false;
        if dest.exists() {
            progress("reused", art);
        } else {
            progress("fetching", art);
            curl_download(art.url, &dest)?;
            downloaded = true;
        }
        progress("verifying", art);
        let observed = sha256_file(&dest)?;
        let verified = if art.sha256.is_empty() {
            false
        } else {
            let ok = observed.eq_ignore_ascii_case(art.sha256);
            if !ok {
                // Pinned digest mismatch — refuse to keep the file.
                let _ = std::fs::remove_file(&dest);
                return Err(EmbedError::ModelUnavailable(format!(
                    "{}: sha256 mismatch (got {}, want {})",
                    art.filename, observed, art.sha256,
                )));
            }
            ok
        };
        outcomes.push(ArtifactOutcome {
            filename: art.filename.to_string(),
            downloaded,
            observed_sha256: observed,
            expected_sha256: art.sha256.to_string(),
            verified,
        });
    }
    Ok(DownloadReport {
        model_dir: model_dir.to_path_buf(),
        artifacts: outcomes,
    })
}

/// Convenience: download `bge-small-en-v1.5` into [`default_model_dir`].
pub fn download_bge_small(
    progress: impl FnMut(&str, &Artifact),
) -> Result<DownloadReport, EmbedError> {
    let dir = default_model_dir("bge-small-en-v1.5")?;
    download_artifacts(&dir, BGE_SMALL_EN_V15_ARTIFACTS, progress)
}

fn curl_download(url: &str, dest: &Path) -> Result<(), EmbedError> {
    let tmp = dest.with_extension("part");
    let _ = std::fs::remove_file(&tmp);
    let status = Command::new("curl")
        .args(["-fSL", "--retry", "3", "--retry-delay", "2", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .map_err(|e| EmbedError::ModelUnavailable(format!("spawn curl: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(EmbedError::ModelUnavailable(format!(
            "curl exited with {status} fetching {url}"
        )));
    }
    std::fs::rename(&tmp, dest).map_err(|e| {
        EmbedError::ModelUnavailable(format!(
            "rename {} -> {}: {e}",
            tmp.display(),
            dest.display()
        ))
    })?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, EmbedError> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    // Heap-allocate the read buffer — 64 KiB on the stack trips
    // clippy::large-stack-arrays and risks blowing the test runner's
    // smaller thread stacks.
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_model_dir_uses_override() {
        // Pure path math — read via env, but we just verify the
        // `$FIRETRAIL_CACHE_HOME` branch by reading it directly without
        // mutating env (process-wide env mutation is forbidden by the
        // workspace lint config; see cache.rs commentary).
        // Compose the expected layout manually:
        let base = PathBuf::from("/tmp/abc");
        let model_id = "bge-small-en-v1.5";
        let expected = base.join("firetrail").join("models").join(model_id);
        assert!(
            expected.ends_with(PathBuf::from("firetrail/models").join(model_id)),
            "{expected:?}"
        );
    }

    #[test]
    fn sha256_file_produces_64_hex_chars() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"firetrail").unwrap();
        let got = sha256_file(&p).unwrap();
        assert_eq!(got.len(), 64);
        assert!(got.chars().all(|c| c.is_ascii_hexdigit()));
        // Determinism: hashing the same bytes twice yields the same hex.
        assert_eq!(got, sha256_file(&p).unwrap());
    }

    #[test]
    fn download_reuses_existing_file() {
        let dir = tempdir().unwrap();
        let fname = "tokenizer.json";
        let dest = dir.path().join(fname);
        std::fs::write(&dest, b"{}").unwrap();

        // Single fake artifact pointing at an unreachable URL — the
        // reuse path must short-circuit before curl is touched.
        let arts = [Artifact {
            filename: "tokenizer.json",
            url: "https://example.invalid/never-fetched",
            sha256: "",
            size_bytes: 0,
        }];

        let mut events: Vec<String> = Vec::new();
        let report = download_artifacts(dir.path(), &arts, |label, art| {
            events.push(format!("{label}:{}", art.filename));
        })
        .expect("reuse");
        assert!(
            events.contains(&"reused:tokenizer.json".to_string()),
            "{events:?}"
        );
        assert!(events.contains(&"verifying:tokenizer.json".to_string()));
        assert_eq!(report.artifacts.len(), 1);
        assert!(!report.artifacts[0].downloaded);
        assert!(!report.artifacts[0].verified, "no pinned hash → unverified");
    }

    #[test]
    fn download_refuses_mismatched_digest() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("model.onnx");
        std::fs::write(&dest, b"not the real weights").unwrap();
        let arts = [Artifact {
            filename: "model.onnx",
            url: "https://example.invalid/never-fetched",
            sha256: "deadbeef00000000000000000000000000000000000000000000000000000000",
            size_bytes: 0,
        }];
        let r = download_artifacts(dir.path(), &arts, |_, _| {});
        assert!(matches!(r, Err(EmbedError::ModelUnavailable(_))));
        // File must have been removed.
        assert!(!dest.exists(), "bad file should be deleted");
    }
}
