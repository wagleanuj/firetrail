//! Shared helpers for the work-graph commands.
//!
//! Every work-graph subcommand follows the same setup pattern:
//!
//! 1. Locate the workspace.
//! 2. Open [`EmbeddedStorage`].
//! 3. Resolve an [`Identity`] for the actor.
//! 4. (Optionally) open the [`Index`] and refresh it after writes.
//!
//! This module centralises that plumbing so each command file stays focused on
//! its domain logic.
//!
//! ### Prefix resolution (ADR-0015)
//!
//! The CLI accepts either the full 69-char `<KIND>-<hex>` form or an
//! unambiguous prefix. [`resolve_record_id`] performs a simple "scan storage,
//! find unique candidate" pass — fine for M1 record counts; a tighter
//! index-backed resolver is filed as `firetrail-exs`.
//!
//! ### Interim relation store
//!
//! `link` / `dep` writes go to `.firetrail/relations.jsonl` (append-only,
//! newline-delimited JSON [`Relation`] records). `show` / `graph` re-read this
//! file to surface external relations alongside the structural ones the index
//! derives from `parent_epic` / `parent_task` / `child_ids`. The canonical
//! relation store is `firetrail-tq7`; until then, this file is the truth.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use ft_core::{Identity, Record, RecordId, Relation, state_hash};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_index::Index;
use ft_storage::{EmbeddedStorage, Storage as _, StorageFilter};

use crate::error::CliError;
use crate::workspace::{self, Workspace};

/// Bundle of resources every work-graph command needs.
pub struct WorkCtx {
    /// Resolved workspace.
    pub ws: Workspace,
    /// On-disk record store.
    pub storage: EmbeddedStorage,
    /// Index handle.
    pub index: Index,
    /// Non-fatal warnings to surface in the JSON envelope.
    pub warnings: Vec<String>,
    /// Identity to stamp on writes (lazily computed on first call).
    actor: Option<Identity>,
    /// Command name for error framing.
    command: String,
}

impl WorkCtx {
    /// Open the workspace, storage, and index for `command`.
    ///
    /// If the index database is missing or fails to open (e.g. corrupt
    /// schema), the index is silently rebuilt from storage. A warning is
    /// emitted so the recovery is observable via the JSON envelope.
    pub fn open(command: &str, override_path: Option<&Path>) -> Result<Self, CliError> {
        let ws = workspace::require_initialised(command, override_path)?;
        let storage = EmbeddedStorage::open(&ws.root)
            .map_err(|e| CliError::internal(command, format!("open storage: {e}")))?;

        let mut warnings = Vec::new();
        let db_path = ws.index_db_path();
        let needs_rebuild = !db_path.exists();
        let mut index = match Index::open(&ws.root) {
            Ok(idx) => idx,
            Err(e) => {
                // Corrupt schema or unmigratable file: nuke and reopen.
                let _ = std::fs::remove_file(&db_path);
                warnings.push(format!(
                    "index.db could not be opened ({e}); rebuilding from storage"
                ));
                Index::open(&ws.root)
                    .map_err(|e| CliError::internal(command, format!("reopen index: {e}")))?
            }
        };
        // Empty schema_version => freshly created or migrations missing.
        let needs_rebuild = needs_rebuild || index.schema_version() == 0;
        if needs_rebuild {
            if !warnings
                .iter()
                .any(|w| w.contains("rebuilding from storage"))
            {
                warnings.push("index.db was missing; rebuilt from storage".to_string());
            }
            index
                .rebuild_from(&storage)
                .map_err(|e| CliError::internal(command, format!("rebuild index: {e}")))?;
            tracing::debug!("auto-rebuilt missing/corrupt index for `{command}`");
        }

        Ok(Self {
            ws,
            storage,
            index,
            warnings,
            actor: None,
            command: command.to_string(),
        })
    }

    /// Resolve and cache the identity of the actor.
    pub fn actor(&mut self) -> Result<Identity, CliError> {
        if let Some(id) = &self.actor {
            return Ok(id.clone());
        }
        let resolver = DefaultResolver::new(&self.ws.root, false);
        let id = resolver.resolve().map_err(|e| CliError::UserError {
            command: self.command.clone(),
            message: format!("identity unresolvable: {e}"),
            details: serde_json::json!({ "hint": "set FIRETRAIL_AUTHOR or `git config user.email`" }),
        })?;
        self.actor = Some(id.clone());
        Ok(id)
    }

    /// Resolve an id string (full or prefix) against on-disk storage.
    pub fn resolve_id(&self, raw: &str) -> Result<RecordId, CliError> {
        resolve_record_id(&self.command, &self.storage, raw)
    }

    /// Persist `record`, recomputing its `state_hash` first, and refresh the
    /// index so the change is queryable immediately.
    pub fn save_record(&mut self, record: &mut Record) -> Result<PathBuf, CliError> {
        record.envelope.state_hash = String::new();
        let new_hash = state_hash(record)
            .map_err(|e| CliError::internal(&self.command, format!("hash: {e}")))?;
        record.envelope.state_hash = new_hash;

        let path = self
            .storage
            .write(record)
            .map_err(|e| CliError::internal(&self.command, format!("write: {e}")))?;

        // The index may have changed shape (status, claim, AC, …); a targeted
        // refresh is cheap and avoids rebuilds.
        self.index
            .refresh(&self.storage, std::slice::from_ref(&path), &[])
            .map_err(|e| CliError::internal(&self.command, format!("refresh: {e}")))?;
        Ok(path)
    }

    /// Read a record by id from storage.
    pub fn read_record(&self, id: &RecordId) -> Result<Record, CliError> {
        self.storage.read(id).map_err(|e| match e {
            ft_storage::StorageError::NotFound(_) => CliError::NotFound {
                command: self.command.clone(),
                what: id.as_str().to_string(),
            },
            other => CliError::internal(&self.command, other),
        })
    }
}

/// Resolve `raw` as either a full record id or a unique prefix.
pub fn resolve_record_id(
    command: &str,
    storage: &EmbeddedStorage,
    raw: &str,
) -> Result<RecordId, CliError> {
    // Fast path: already a valid full id.
    if let Ok(id) = RecordId::from_string(raw.to_string()) {
        // Verify it exists.
        if storage.read(&id).is_ok() {
            return Ok(id);
        }
        return Err(CliError::NotFound {
            command: command.to_string(),
            what: raw.to_string(),
        });
    }

    // Slow path: walk storage and find unique candidate where the canonical id
    // (uppercased) starts with `raw` (uppercased) up to the kind separator.
    let needle = raw.to_ascii_lowercase();
    let candidates = storage.list(&StorageFilter::default()).map_err(|e| {
        CliError::internal(command, format!("scanning storage for prefix match: {e}"))
    })?;
    let matches: Vec<RecordId> = candidates
        .into_iter()
        .filter(|id| id.as_str().to_ascii_lowercase().starts_with(&needle))
        .collect();
    match matches.len() {
        0 => Err(CliError::NotFound {
            command: command.to_string(),
            what: raw.to_string(),
        }),
        1 => Ok(matches.into_iter().next().expect("len==1")),
        _ => Err(CliError::UserError {
            command: command.to_string(),
            message: format!("`{raw}` is ambiguous; matches multiple records"),
            details: serde_json::json!({
                "ambiguous_prefix": raw,
                "matches": matches.iter().map(|m| m.as_str().to_string()).collect::<Vec<_>>(),
            }),
        }),
    }
}

/// Path of the interim relation log.
#[must_use]
pub fn relations_log_path(ws: &Workspace) -> PathBuf {
    ws.firetrail_dir().join("relations.jsonl")
}

/// Append a relation to the interim log.
pub fn append_relation(ws: &Workspace, relation: &Relation) -> Result<(), CliError> {
    let path = relations_log_path(ws);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::internal("link", format!("ensure relations dir: {e}")))?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| CliError::internal("link", format!("open relations log: {e}")))?;
    let line = serde_json::to_string(relation)
        .map_err(|e| CliError::internal("link", format!("encode relation: {e}")))?;
    writeln!(f, "{line}")
        .map_err(|e| CliError::internal("link", format!("write relations log: {e}")))?;
    Ok(())
}

/// Load every relation currently in the log.
pub fn load_relations(ws: &Workspace) -> Result<Vec<Relation>, CliError> {
    let path = relations_log_path(ws);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = std::fs::File::open(&path)
        .map_err(|e| CliError::internal("show", format!("open relations log: {e}")))?;
    let mut out = Vec::new();
    for (lineno, line) in BufReader::new(f).lines().enumerate() {
        let line =
            line.map_err(|e| CliError::internal("show", format!("read relations line: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let rel: Relation = serde_json::from_str(&line).map_err(|e| {
            CliError::internal("show", format!("parse relations line {}: {e}", lineno + 1))
        })?;
        out.push(rel);
    }
    Ok(out)
}

/// Overwrite the relation log with `relations`.
pub fn rewrite_relations(ws: &Workspace, relations: &[Relation]) -> Result<(), CliError> {
    let path = relations_log_path(ws);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::internal("dep", format!("ensure relations dir: {e}")))?;
    }
    let mut s = String::new();
    for r in relations {
        let line = serde_json::to_string(r)
            .map_err(|e| CliError::internal("dep", format!("encode relation: {e}")))?;
        s.push_str(&line);
        s.push('\n');
    }
    std::fs::write(&path, s)
        .map_err(|e| CliError::internal("dep", format!("rewrite relations log: {e}")))?;
    Ok(())
}

/// Parse a `key=value` label argument; returns the (key, value) pair or a user
/// error if the `=` separator is missing.
pub fn parse_label_pair(command: &str, raw: &str) -> Result<(String, String), CliError> {
    let (k, v) = raw.split_once('=').ok_or_else(|| CliError::UserError {
        command: command.to_string(),
        message: format!("label `{raw}` must be in `key=value` form"),
        details: serde_json::json!({ "label": raw }),
    })?;
    if k.trim().is_empty() {
        return Err(CliError::UserError {
            command: command.to_string(),
            message: format!("label key in `{raw}` is empty"),
            details: serde_json::Value::Null,
        });
    }
    Ok((k.trim().to_string(), v.trim().to_string()))
}
