//! Cross-repo reference enforcement for external mode (ADR-0010).
//!
//! In external storage mode, code-repo commits may reference records that
//! live in a separate data repo. [`validate_external_references`] walks the
//! commits in `base..head` of the code repo, extracts record-id references
//! from each commit message, and reports any references whose target record
//! cannot be found in the data storage.
//!
//! The recognized footer / mention patterns mirror those validated by
//! `ft-pr`:
//!
//! - `firetrail-closes: TASK-7f2a91`
//! - `firetrail-relates: INC-abc123`
//! - `closes TASK-7f2a91` / `fixes BUG-...` / `resolves FIND-...`
//!
//! References are case-insensitive. Whitespace is tolerated. A single commit
//! may reference multiple records; each reference is validated independently.

use std::process::Command;

use ft_core::RecordId;
use regex::Regex;

use crate::StorageError;
use crate::storage::Storage;

/// A single unresolved cross-repo reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRefViolation {
    /// SHA of the commit whose message contained the reference.
    pub commit_sha: String,
    /// The referenced record id that could not be resolved.
    pub record_id: RecordId,
    /// Human-readable explanation.
    pub reason: String,
}

/// Walk commits in `base..head` of `code_repo_git` and verify that every
/// record id referenced by a commit message exists in `data_storage`.
///
/// Returns the list of unresolved references. An empty vector means every
/// reference resolved cleanly.
///
/// Implementation notes:
/// - Uses `git log --format=%H%x00%B%x1e base..head` against the code repo's
///   root. Shell failures (shallow clones, missing refs) are reported as a
///   single violation against `base..head` rather than panicking; callers
///   surface this as a check failure.
/// - Only `read` is used against `data_storage`, so any [`Storage`] impl —
///   embedded or external — works. In practice external mode is the
///   interesting case.
pub fn validate_external_references(
    code_repo_git: &ft_git::Repo,
    data_storage: &dyn Storage,
    base: &str,
    head: &str,
) -> Vec<ExternalRefViolation> {
    let mut out = Vec::new();
    let commits = match collect_commits(code_repo_git, base, head) {
        Ok(c) => c,
        Err(reason) => {
            // We cannot manufacture a RecordId without parsing; surface as a
            // structural failure pinned to the range itself.
            out.push(ExternalRefViolation {
                commit_sha: format!("{base}..{head}"),
                record_id: synthetic_unknown_id(),
                reason: format!("failed to read commit range: {reason}"),
            });
            return out;
        }
    };

    let footer_re =
        Regex::new(r"(?i)\bfiretrail-(?:closes|relates|fixes|resolves):\s*([A-Z]+-[0-9a-fA-F]+)")
            .ok();
    let plain_re = Regex::new(r"(?i)\b(?:closes|fixes|resolves):?\s+([A-Z]+-[0-9a-fA-F]+)").ok();

    for (sha, message) in commits {
        let mut seen: Vec<RecordId> = Vec::new();
        for re in [&footer_re, &plain_re].iter().copied().flatten() {
            for cap in re.captures_iter(&message) {
                if let Some(m) = cap.get(1) {
                    if let Ok(id) = RecordId::from_string(m.as_str().to_string()) {
                        if !seen.iter().any(|x| x.as_str() == id.as_str()) {
                            seen.push(id);
                        }
                    }
                }
            }
        }
        for id in seen {
            match data_storage.read(&id) {
                Ok(_) => {}
                Err(StorageError::NotFound(_)) => {
                    out.push(ExternalRefViolation {
                        commit_sha: sha.clone(),
                        record_id: id.clone(),
                        reason: format!(
                            "commit {sha} references {} but no such record exists in the data repo",
                            id.as_str()
                        ),
                    });
                }
                Err(e) => {
                    out.push(ExternalRefViolation {
                        commit_sha: sha.clone(),
                        record_id: id.clone(),
                        reason: format!(
                            "commit {sha} references {} but read failed: {e}",
                            id.as_str()
                        ),
                    });
                }
            }
        }
    }
    out
}

fn collect_commits(
    repo: &ft_git::Repo,
    base: &str,
    head: &str,
) -> Result<Vec<(String, String)>, String> {
    let range = format!("{base}..{head}");
    let output = Command::new("git")
        .args(["log", "--format=%H%x00%B%x1e", &range])
        .current_dir(repo.root())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut commits = Vec::new();
    for chunk in raw.split('\u{1e}') {
        let chunk = chunk.trim_matches(['\n', '\r', ' ']);
        if chunk.is_empty() {
            continue;
        }
        let mut parts = chunk.splitn(2, '\u{0}');
        let sha = parts.next().unwrap_or("").trim().to_string();
        let body = parts.next().unwrap_or("").to_string();
        if !sha.is_empty() {
            commits.push((sha, body));
        }
    }
    Ok(commits)
}

/// Synthetic id used when the range itself cannot be read; never compared
/// against real records.
fn synthetic_unknown_id() -> RecordId {
    // RecordId::from_string accepts any KIND-hex shape; use a sentinel that
    // can't collide with a real record. If parsing somehow fails, fall back
    // to constructing via mint() so we always have a valid id.
    RecordId::from_string("MEMORY-0000000000000000".to_string()).unwrap_or_else(|_| {
        use ft_core::{Identity, RecordKind};
        RecordId::mint(
            RecordKind::Memory,
            &Identity::new("firetrail@local").expect("static identity"),
        )
    })
}
