//! Internal helpers shared by every memory op.
//!
//! Mirrors `super::tickets::ctx::TicketCtx` but scoped for memory writes:
//! same open semantics (storage + index + identity strict-mode check), and
//! the same `save_record` path that:
//!
//! - appends a `Create`/`Update` history entry via `ft-history`,
//! - refreshes the relational index,
//! - upserts the search FTS,
//! - best-effort dispatches an embed-on-write to the daemon socket.
//!
//! Not public — adapters interact through the typed op signatures only.

use std::fs;
use std::path::{Path, PathBuf};

use ft_core::{
    Identity as CoreIdentity, Record, RecordId, ResolveError, resolve_prefix, state_hash,
};
use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
use ft_identity::load_registry;
use ft_index::Index;
use ft_search::SearchEngine;
use ft_storage::{EmbeddedStorage, ExternalStorage, Storage as _, StorageFilter};

use crate::error::OpsError;
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Internal bundle of resources every memory op needs.
pub(crate) struct MemoryCtx<'a> {
    pub ws: &'a Workspace,
    /// Record store. In external mode this is rooted on the data-repo clone,
    /// not the host workspace (firetrail-zkme).
    pub storage: EmbeddedStorage,
    /// External-mode handle. `Some` when storage mode is external; writes are
    /// routed through it so they auto-commit into the data-repo clone.
    pub external: Option<ExternalStorage>,
    pub index: Index,
    /// Stable name used in history summaries (e.g. `"incident create"`).
    pub op: &'static str,
    /// Resolved actor for this invocation.
    pub actor: CoreIdentity,
    search: Option<SearchEngine>,
}

impl<'a> MemoryCtx<'a> {
    /// Open storage + index for `op`, validate the caller against the
    /// identity registry when strict mode is on, and return a ready context.
    pub fn open(
        ws: &'a Workspace,
        identity: &Identity,
        op: &'static str,
    ) -> Result<Self, OpsError> {
        let (storage, external) = ft_storage::resolve_workspace_storage(&ws.root)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("open storage: {e}")))?;
        if let Some(ext) = &external {
            if let Err(e) = ext.pull() {
                tracing::warn!(error = %e, op = op, "external storage auto-pull failed");
            }
        }

        let db_path = ws.index_db_path();
        let needs_rebuild = !db_path.exists();
        let mut index = match Index::open(&ws.root) {
            Ok(idx) => idx,
            Err(e) => {
                let _ = fs::remove_file(&db_path);
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
            external,
            index,
            op,
            actor,
            search: None,
        })
    }

    /// Resolve a (full or prefix) record id against on-disk storage.
    pub fn resolve_id(&self, raw: &str) -> Result<RecordId, OpsError> {
        if let Ok(id) = RecordId::from_string(raw.to_string()) {
            return if self.storage.read(&id).is_ok() {
                Ok(id)
            } else {
                Err(OpsError::not_found("memory", raw.to_string()))
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
            Err(ResolveError::Unknown(_)) => Err(OpsError::not_found("memory", raw.to_string())),
            Err(ResolveError::Ambiguous { matches, .. }) => Err(OpsError::Conflict {
                reason: format!("`{raw}` is ambiguous; matches {} records", matches.len()),
            }),
        }
    }

    /// Read a record from storage, mapping `NotFound` cleanly.
    pub fn read_record(&self, id: &RecordId) -> Result<Record, OpsError> {
        self.storage.read(id).map_err(|e| match e {
            ft_storage::StorageError::NotFound(_) => {
                OpsError::not_found("memory", id.as_str().to_string())
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

    /// Persist `record` after appending the caller-supplied `draft` history
    /// entry. Used by trust transitions where the caller must pick the
    /// `HistoryEntryKind::TrustTransition` kind explicitly.
    pub fn save_record_with_history(
        &mut self,
        record: &mut Record,
        draft: HistoryDraft,
    ) -> Result<PathBuf, OpsError> {
        append_history(record, draft)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("history append: {e}")))?;
        self.persist_record(record)
    }

    fn persist_record(&mut self, record: &mut Record) -> Result<PathBuf, OpsError> {
        record.envelope.state_hash = String::new();
        let new_hash =
            state_hash(record).map_err(|e| OpsError::Internal(anyhow::anyhow!("hash: {e}")))?;
        record.envelope.state_hash = new_hash;

        // External mode: route through ExternalStorage so the write auto-commits
        // into the data-repo clone (firetrail-zkme).
        let path = if let Some(ext) = &self.external {
            ext.write(record)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("write (external): {e}")))?
        } else {
            self.storage
                .write(record)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("write: {e}")))?
        };

        self.index
            .refresh(&self.storage, std::slice::from_ref(&path), &[])
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("refresh: {e}")))?;

        self.upsert_search_lexical(record);
        self.try_dispatch_index_record(record);
        self.upsert_and_embed_audit_docs(record);
        Ok(path)
    }

    /// firetrail-8z0m.5: keep the record's `audit:<id>#h<n>` synthetic docs
    /// current on write — append-of-history happens in `save_record`, so a save
    /// always produces at least one new audit entry. Upsert each audit doc
    /// lexically (FTS + meta) and dispatch its embedding under the audit `DocId`,
    /// mirroring how `index rebuild` indexes + embeds these docs. Non-fatal: a
    /// search-layer hiccup must never block the write.
    fn upsert_and_embed_audit_docs(&mut self, record: &Record) {
        let docs = crate::synthetic_embed::audit_docs_for(record);
        if docs.is_empty() {
            return;
        }
        let op = self.op;
        match self.search_engine() {
            Ok(engine) => {
                for doc in &docs {
                    if let Err(e) = engine.upsert_document(doc) {
                        tracing::warn!(error = %e, op = op, "audit doc upsert failed");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, op = op, "search engine unavailable for audit docs");
            }
        }
        crate::synthetic_embed::dispatch_docs(self.ws, self.op, &docs);
    }

    /// Persist `record` after auto-appending a history entry (Create on
    /// genesis, Update otherwise) and refreshing index + search.
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
            transition: None,
        };
        append_history(record, draft)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("history append: {e}")))?;
        self.persist_record(record)
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

    /// Borrow the search engine for read-only queries. Used by [`super::search`].
    pub fn read_search_engine(&mut self) -> Result<&SearchEngine, OpsError> {
        self.search_engine()
    }
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
