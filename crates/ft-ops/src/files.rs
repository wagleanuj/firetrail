//! Tracked-path listing — backs the ft-ui file-path autocomplete.
//!
//! [`list_files`] reads the set of files tracked at `HEAD` (via
//! [`ft_git::Repo::list_files_at_ref`]), normalizes them to forward-slash
//! repo-relative strings, optionally collapses them to the distinct set of
//! ancestor directories (`dirs_only`), filters case-insensitively by `prefix`,
//! and returns a sorted, deduped, length-capped `Vec<String>`.
//!
//! Like every op in this crate it is transport-agnostic: no HTTP/CLI types, no
//! ambient context. The route/CLI layer supplies `prefix`, `dirs_only`, and
//! `limit`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::workspace::Workspace;

/// The autocomplete result: a flat list of repo-relative, forward-slash paths.
///
/// The wire mirror surfaced by `GET /api/files`, kept in `ft-ops` so ts-rs only
/// ever sees ops types (the xtask ts exporter depends on `ft-ops`, not `ft-ui`).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileListView {
    /// Suggested paths (or directory prefixes), sorted and deduped.
    pub paths: Vec<String>,
}

/// Upper bound on how many suggestions [`list_files`] will return; callers that
/// pass a larger `limit` are clamped to this.
const MAX_LIMIT: usize = 200;

/// List tracked repo paths (or their ancestor directories) matching `prefix`.
///
/// - Reads the tree at `HEAD` and converts each path to a forward-slash
///   repo-relative string.
/// - When `dirs_only` is `true`, the result is the **distinct set of ancestor
///   directories** of every tracked file (so component paths such as
///   `crates/ft-cli` are suggestible); file paths themselves are dropped.
/// - Filters to entries that start with `prefix` (case-insensitive; an empty
///   `prefix` matches everything).
/// - Sorts, dedupes, and truncates to `limit`, which is clamped to
///   `1..=MAX_LIMIT`.
///
/// # Errors
///
/// Returns [`OpsError::Internal`] when the git tree at `HEAD` cannot be read.
pub fn list_files(
    ws: &Workspace,
    prefix: &str,
    dirs_only: bool,
    limit: usize,
) -> Result<Vec<String>, OpsError> {
    let limit = limit.clamp(1, MAX_LIMIT);

    let repo = ft_git::Repo::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open repo: {e}")))?;
    let tracked = repo
        .list_files_at_ref("HEAD", "**")
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list files at HEAD: {e}")))?;

    // Forward-slash, repo-relative strings.
    let files: Vec<String> = tracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    // The candidate set is either the directory prefixes or the files
    // themselves.
    let mut candidates: BTreeSet<String> = if dirs_only {
        let mut dirs = BTreeSet::new();
        for f in &files {
            // Every ancestor directory of the file is a suggestible prefix.
            let mut acc = String::new();
            for comp in f.split('/').rev().skip(1).collect::<Vec<_>>().iter().rev() {
                if acc.is_empty() {
                    acc.push_str(comp);
                } else {
                    acc.push('/');
                    acc.push_str(comp);
                }
                dirs.insert(acc.clone());
            }
        }
        dirs
    } else {
        files.into_iter().collect()
    };

    // Case-insensitive prefix filter (empty prefix matches all).
    if !prefix.is_empty() {
        let needle = prefix.to_ascii_lowercase();
        candidates.retain(|c| c.to_ascii_lowercase().starts_with(&needle));
    }

    // BTreeSet is already sorted + deduped; just cap the length.
    Ok(candidates.into_iter().take(limit).collect())
}
