//! Internal helpers shared by every ticket op.
//!
//! Mirrors `ft_cli::context::WorkCtx` but returns [`OpsError`] instead of
//! `CliError`. This is intentionally not pub — adapters never touch it; they
//! only call the typed ops exported from `super`.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};

use ft_core::{
    Identity as CoreIdentity, Record, RecordId, Relation, ResolveError, resolve_prefix, state_hash,
};
use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
use ft_identity::load_registry;
use ft_index::Index;
use ft_search::SearchEngine;
use ft_storage::{EmbeddedStorage, Storage as _, StorageFilter};

use crate::error::OpsError;
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Internal bundle of resources every ticket op needs.
pub(crate) struct TicketCtx<'a> {
    pub ws: &'a Workspace,
    pub storage: EmbeddedStorage,
    pub index: Index,
    /// Stable name used in lockfiles + history summaries.
    pub op: &'static str,
    /// Resolved actor for this invocation.
    pub actor: CoreIdentity,
    search: Option<SearchEngine>,
}

impl<'a> TicketCtx<'a> {
    /// Open storage + index for `op`, validate the caller against the
    /// identity registry when strict mode is on, and return a ready
    /// context.
    pub fn open(
        ws: &'a Workspace,
        identity: &Identity,
        op: &'static str,
    ) -> Result<Self, OpsError> {
        let storage = EmbeddedStorage::open(&ws.root)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("open storage: {e}")))?;

        let db_path = ws.index_db_path();
        let needs_rebuild = !db_path.exists();
        let mut index = match Index::open(&ws.root) {
            Ok(idx) => idx,
            Err(e) => {
                let _ = std::fs::remove_file(&db_path);
                tracing::warn!(error = %e, op = op, "index.db reopened after error");
                Index::open(&ws.root)
                    .map_err(|e| OpsError::Internal(anyhow::anyhow!("reopen index: {e}")))?
            }
        };
        let needs_rebuild = needs_rebuild || index.schema_version() == 0;
        if needs_rebuild {
            index
                .rebuild_from(&storage)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("rebuild index: {e}")))?;
        }

        let actor = identity.to_core()?;
        if strict_identity_enabled(&ws.root) {
            let registry = load_registry(&ws.root)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("load identity registry: {e}")))?;
            if registry.resolve_canonical(actor.as_str()).is_none() {
                return Err(OpsError::PermissionDenied {
                    reason: format!(
                        "identity `{}` is not registered in strict-mode workspace",
                        actor.as_str()
                    ),
                });
            }
        }

        Ok(Self {
            ws,
            storage,
            index,
            op,
            actor,
            search: None,
        })
    }

    /// Resolve a (full or prefix) id against on-disk storage.
    pub fn resolve_id(&self, raw: &str) -> Result<RecordId, OpsError> {
        if let Ok(id) = RecordId::from_string(raw.to_string()) {
            return if self.storage.read(&id).is_ok() {
                Ok(id)
            } else {
                Err(OpsError::not_found("ticket", raw.to_string()))
            };
        }
        let candidates = self
            .storage
            .list(&StorageFilter::default())
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("scan storage: {e}")))?;
        match resolve_prefix(raw, &candidates) {
            Ok(id) => Ok(id),
            Err(ResolveError::Empty) => Err(OpsError::validation("id", "empty record id")),
            Err(ResolveError::EmptyHexPrefix(kind)) => Err(OpsError::validation(
                "id",
                format!("hex prefix is required after kind tag `{kind}`"),
            )),
            Err(ResolveError::Unknown(_)) => Err(OpsError::not_found("ticket", raw.to_string())),
            Err(ResolveError::Ambiguous { matches, .. }) => Err(OpsError::Conflict {
                reason: format!("`{raw}` is ambiguous; matches {} records", matches.len()),
            }),
        }
    }

    /// Read a record from storage, mapping `NotFound` cleanly.
    pub fn read_record(&self, id: &RecordId) -> Result<Record, OpsError> {
        self.storage.read(id).map_err(|e| match e {
            ft_storage::StorageError::NotFound(_) => {
                OpsError::not_found("ticket", id.as_str().to_string())
            }
            other => OpsError::Internal(anyhow::anyhow!("read record: {other}")),
        })
    }

    fn search_engine(&mut self) -> Result<&SearchEngine, OpsError> {
        if self.search.is_none() {
            let db = self.index.db_path().to_path_buf();
            let engine = SearchEngine::open(&db)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("open search: {e}")))?;
            engine
                .ensure_schema()
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("ensure search schema: {e}")))?;
            self.search = Some(engine);
        }
        Ok(self.search.as_ref().expect("just set"))
    }

    /// Persist `record` after auto-appending a history entry (genesis Create
    /// or subsequent Update) and refreshing index + search.
    pub fn save_record(&mut self, record: &mut Record) -> Result<PathBuf, OpsError> {
        let kind =
            if record.envelope.history.is_empty() && record.envelope.prev_state_hash.is_none() {
                HistoryEntryKind::Create
            } else {
                HistoryEntryKind::Update
            };
        let kind_tag = record.envelope.kind.prefix().to_ascii_lowercase();
        let summary = match kind {
            HistoryEntryKind::Create => format!("{kind_tag} created via `{}`", self.op),
            _ => format!("{kind_tag} updated via `{}`", self.op),
        };
        let draft = HistoryDraft {
            merged_via_pr: None,
            timestamp: record.envelope.updated_at,
            primary_actor: self.actor.clone(),
            contributors: Vec::new(),
            ops_summary: vec![summary],
            ops_count: 1,
            kind,
        };
        append_history(record, draft)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("history append: {e}")))?;

        self.persist_record(record)
    }

    fn persist_record(&mut self, record: &mut Record) -> Result<PathBuf, OpsError> {
        record.envelope.state_hash = String::new();
        let new_hash =
            state_hash(record).map_err(|e| OpsError::Internal(anyhow::anyhow!("hash: {e}")))?;
        record.envelope.state_hash = new_hash;

        let path = self
            .storage
            .write(record)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("write: {e}")))?;

        self.index
            .refresh(&self.storage, std::slice::from_ref(&path), &[])
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("refresh: {e}")))?;

        self.upsert_search_lexical(record);
        self.try_dispatch_index_record(record);

        Ok(path)
    }

    fn upsert_search_lexical(&mut self, record: &Record) {
        let op = self.op;
        let root = self.ws.root.clone();
        match self.search_engine() {
            Ok(engine) => {
                if let Err(e) = engine.upsert_lexical_with_root(record, &root) {
                    tracing::warn!(error = %e, op = op, "search upsert failed");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, op = op, "search engine unavailable");
            }
        }
    }

    fn try_dispatch_index_record(&self, record: &Record) {
        let socket = match self.ws.daemon_socket_path() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, op = self.op, "resolve daemon socket path");
                return;
            }
        };
        if ft_embed::daemon::status(&socket) != ft_embed::DaemonStatus::Running {
            return;
        }
        let text = ft_embed::record_text_with_root(&self.ws.root, record);
        if let Err(e) =
            ft_embed::daemon::send_index_record(&socket, record.envelope.id.as_str(), &text)
        {
            tracing::warn!(error = %e, op = self.op, "embed-on-write dispatch failed");
        }
    }
}

/// RAII lockfile guard used by [`claim`] and [`close`].
pub(crate) struct LockHandle {
    path: PathBuf,
}

impl LockHandle {
    pub fn acquire(ws: &Workspace, id: &RecordId, suffix: &str) -> Result<Self, OpsError> {
        let lower = id.as_str().to_lowercase();
        let path = ws
            .firetrail_dir()
            .join("locks")
            .join(format!("{lower}.{suffix}"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("locks dir: {e}")))?;
        }
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(Self { path }),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(OpsError::Conflict {
                reason: format!("another `{suffix}` is in-flight for {}", id.as_str()),
            }),
            Err(e) => Err(OpsError::Internal(anyhow::anyhow!("lockfile error: {e}"))),
        }
    }
}

impl Drop for LockHandle {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Relations log (interim store mirroring ft-cli::context).
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn relations_log_path(ws: &Workspace) -> PathBuf {
    ws.firetrail_dir().join("relations.jsonl")
}

pub(crate) fn append_relation(ws: &Workspace, relation: &Relation) -> Result<(), OpsError> {
    let path = relations_log_path(ws);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("ensure relations dir: {e}")))?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open relations log: {e}")))?;
    let line = serde_json::to_string(relation)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("encode relation: {e}")))?;
    writeln!(f, "{line}")
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("write relations log: {e}")))?;
    Ok(())
}

pub(crate) fn load_relations(ws: &Workspace) -> Result<Vec<Relation>, OpsError> {
    let path = relations_log_path(ws);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = fs::File::open(&path)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open relations log: {e}")))?;
    let mut out = Vec::new();
    for (lineno, line) in BufReader::new(f).lines().enumerate() {
        let line =
            line.map_err(|e| OpsError::Internal(anyhow::anyhow!("read relations line: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let rel: Relation = serde_json::from_str(&line).map_err(|e| {
            OpsError::Internal(anyhow::anyhow!("parse relations line {}: {e}", lineno + 1))
        })?;
        out.push(rel);
    }
    Ok(out)
}

fn strict_identity_enabled(root: &Path) -> bool {
    #[derive(serde::Deserialize)]
    struct StrictFlag {
        strict: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct ConfigShell {
        identity: Option<StrictFlag>,
    }
    let path = root.join(".firetrail").join("config.yml");
    let Ok(s) = fs::read_to_string(&path) else {
        return false;
    };
    let parsed: ConfigShell = match serde_yaml::from_str(&s) {
        Ok(v) => v,
        Err(_) => return false,
    };
    parsed.identity.and_then(|i| i.strict).unwrap_or(false)
}

/// Serialize a [`ft_core::RelationKind`] to its kebab-case wire form.
pub(crate) fn relation_kind_str(k: ft_core::RelationKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{k:?}"))
}
